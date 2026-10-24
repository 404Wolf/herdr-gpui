//! Pull requests on Gitea and Forgejo hosts, for checkouts whose remote names
//! a host configured under `[gitea]`. Each batch hands this module its inputs
//! first: those on a Gitea host are answered here, over Gitea's REST API, as
//! the same `PullRequest` values GitHub lookups produce; every other input is
//! passed back untouched for the GraphQL path.
//!
//! A batch reads each repository's open pull requests once, however many of
//! its worktrees are asked about, then each found pull request's detail,
//! commit status, and reviews.

mod build;
#[cfg(test)]
mod tests;

use super::{
    Input, Origin, Result,
    batch::Batch,
    fetch::{TIMEOUT, git, local_checkout, run},
};
use crate::Error;
use herdr_gitea::{Client, Host, MAX_PAGES, PullRequest as GiteaPull, Repo, Repository};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

/// The prefix of the opaque ID a Gitea pull request carries, which tells the
/// action worker to answer it over Gitea rather than GitHub's GraphQL.
pub(crate) const ID_PREFIX: &str = "gitea:";

/// How long a remote device's remotes are trusted before SSH reads them again.
const REMOTE_TTL: Duration = Duration::from_secs(10 * 60);
const REMOTE_LIMIT: usize = 64;

/// A checkout on a Gitea host: the repository its pull requests are opened
/// against, the owner its branch is pushed under, and the branch's name there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Located {
    pub repo: Repo,
    pub head_owner: String,
    pub branch: String,
    /// Whether the base is the `upstream` remote, with `origin` a fork.
    pub upstream: bool,
}

/// Remote devices' `origin`/`upstream` URLs and branch merge refs, keyed by
/// SSH target, Git directory, and branch, so a refresh does not dial again.
type Remotes = Vec<(String, String, String, Instant, Option<Located>)>;
static REMOTES: Mutex<Remotes> = Mutex::new(Vec::new());

/// The Gitea repository a verified local checkout's pull requests and issues
/// belong to, and, when that is its `upstream`, the owner of the fork behind
/// `origin`. `None` for a checkout on no configured Gitea host.
pub fn local_gitea_repository(
    input: &Input,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> crate::Result<Option<(Repo, Option<String>)>> {
    let hosts = herdr_gitea::hosts();
    if hosts.is_empty() {
        return Ok(None);
    }
    Ok(locate(&hosts, input, &Origin::Local, deadline, cancelled)?
        .map(|located| (located.repo, located.upstream.then_some(located.head_owner))))
}

/// Tokens for a window signed out of GitHub while Gitea hosts are configured:
/// their checkouts are read with each host's own token, so the prefetch and
/// actions run without a GitHub sign-in, and GitHub checkouts are not asked.
pub fn signed_out_tokens() -> Option<crate::Tokens> {
    herdr_gitea::configured()
        .then(|| crate::Tokens::from(std::sync::Arc::new(secrecy::SecretString::default())))
}

/// Answers every input on a Gitea host into `batch` and returns the rest.
/// With no host configured nothing is read, so GitHub-only setups pay nothing.
pub(super) fn claim(
    inputs: Vec<Input>,
    origin: &Origin,
    batch: &mut Batch,
    cancelled: &impl Fn() -> bool,
) -> Vec<Input> {
    let hosts = herdr_gitea::hosts();
    if hosts.is_empty() {
        return inputs;
    }
    claim_in(&hosts, inputs, origin, batch, cancelled)
}

/// [`claim`] over an explicit host list.
pub(super) fn claim_in(
    hosts: &[Host],
    inputs: Vec<Input>,
    origin: &Origin,
    batch: &mut Batch,
    cancelled: &impl Fn() -> bool,
) -> Vec<Input> {
    let deadline = Instant::now() + TIMEOUT;
    let mut rest = Vec::new();
    let mut claimed: Vec<(Input, Located)> = Vec::new();
    for input in inputs {
        match locate(hosts, &input, origin, deadline, cancelled) {
            Ok(Some(located)) => claimed.push((input, located)),
            Ok(None) => rest.push(input),
            // A checkout whose remotes cannot be read is the GitHub path's to
            // report, as it was before Gitea hosts existed.
            Err(_) => rest.push(input),
        }
    }
    // One listing per repository, shared by its worktrees.
    while let Some((input, located)) = claimed.pop() {
        let mut group = vec![(input, located)];
        let mut index = 0;
        while index < claimed.len() {
            if claimed[index].1.repo.same(&group[0].1.repo) {
                group.push(claimed.swap_remove(index));
            } else {
                index += 1;
            }
        }
        answer(group, batch, cancelled);
    }
    rest
}

/// Where `input`'s pull requests live, if on a configured Gitea host. Local
/// checkouts are verified like the GitHub path verifies them, but only once a
/// remote says they are Gitea's, so GitHub checkouts run one `git config`.
pub(super) fn locate(
    hosts: &[Host],
    input: &Input,
    origin: &Origin,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> crate::Result<Option<Located>> {
    match origin {
        Origin::Local => {
            let read = |key: &str| local_config(input, key, deadline, cancelled);
            let Some(located) = resolve(hosts, &input.branch, read)? else {
                return Ok(None);
            };
            local_checkout(input, deadline, cancelled)?;
            Ok(Some(located))
        }
        Origin::Ssh(target) => {
            let key = input.repo_key.as_deref().ok_or(Error::PrMetadata)?;
            let now = Instant::now();
            if let Ok(mut remotes) = REMOTES.lock() {
                remotes.retain(|(.., read, _)| now.duration_since(*read) < REMOTE_TTL);
                if let Some((.., located)) = remotes.iter().find(|(host, git_dir, branch, ..)| {
                    host == target && git_dir == key && *branch == input.branch
                }) {
                    return Ok(located.clone());
                }
            }
            let read = |name: &str| {
                let timeout = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or(Error::PrTimeout)?;
                Ok(herdr_client::remote_config_value(
                    target, key, name, timeout, cancelled,
                )?)
            };
            let located = resolve(hosts, &input.branch, read)?;
            if let Ok(mut remotes) = REMOTES.lock() {
                if remotes.len() >= REMOTE_LIMIT {
                    remotes.remove(0);
                }
                remotes.push((
                    target.clone(),
                    key.to_owned(),
                    input.branch.clone(),
                    now,
                    located.clone(),
                ));
            }
            Ok(located)
        }
    }
}

/// The base repository and head of `branch` from Git config values `read`
/// returns. An `upstream` remote on a Gitea host is the base, as a fork
/// checkout has it, with `origin` the fork its branches are pushed to.
pub(super) fn resolve(
    hosts: &[Host],
    branch: &str,
    mut read: impl FnMut(&str) -> crate::Result<Option<String>>,
) -> crate::Result<Option<Located>> {
    let origin = read("remote.origin.url")?
        .as_deref()
        .and_then(|url| herdr_gitea::repo_in(hosts, url));
    let upstream = read("remote.upstream.url")?
        .as_deref()
        .and_then(|url| herdr_gitea::repo_in(hosts, url))
        .filter(|upstream| origin.as_ref().is_none_or(|origin| !origin.same(upstream)));
    let (repo, head_owner, upstream) = match (upstream, origin) {
        (Some(upstream), origin) => {
            let owner = origin.map_or_else(|| upstream.owner.clone(), |origin| origin.owner);
            (upstream, owner, true)
        }
        (None, Some(origin)) => {
            let owner = origin.owner.clone();
            (origin, owner, false)
        }
        (None, None) => return Ok(None),
    };
    // The branch's name on the remote, which a local branch may rename.
    let branch = read(&format!("branch.{branch}.merge"))?
        .as_deref()
        .and_then(|merge| merge.strip_prefix("refs/heads/"))
        .filter(|name| !name.is_empty())
        .map_or_else(|| branch.to_owned(), str::to_owned);
    Ok(Some(Located {
        repo,
        head_owner,
        branch,
        upstream,
    }))
}

/// One local Git config value, `None` when unset.
fn local_config(
    input: &Input,
    key: &str,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> crate::Result<Option<String>> {
    let value = match (&input.repo_key, &input.checkout) {
        (Some(git_dir), _) => {
            let mut command = herdr_client::new_command("git");
            command.args([
                "-c",
                "core.fsmonitor=false",
                "--git-dir",
                git_dir,
                "config",
                "--default",
                "",
                "--get",
                key,
            ]);
            let (ok, output) = run(&mut command, deadline, cancelled)?;
            if !ok {
                return Err(Error::PrCheckout);
            }
            output.trim_end_matches(['\r', '\n']).to_owned()
        }
        (None, Some(checkout)) => git(
            checkout,
            &["config", "--default", "", "--get", key],
            deadline,
            cancelled,
        )?,
        (None, None) => return Err(Error::PrMetadata),
    };
    Ok((!value.is_empty()).then_some(value))
}

/// Answers one repository's inputs: its settings and open pull requests are
/// read once; a failure to read them fails each input alike.
fn answer(group: Vec<(Input, Located)>, batch: &mut Batch, cancelled: &impl Fn() -> bool) {
    let repo = group[0].1.repo.clone();
    let shared = Client::new(&repo.host, TIMEOUT)
        .map_err(from_gitea)
        .and_then(|client| {
            let settings: Repository = client.get(&repo.api(""), cancelled).map_err(from_gitea)?;
            let (open, _complete): (Vec<GiteaPull>, bool) = client
                .get_pages(&repo.api("/pulls?state=open"), MAX_PAGES, cancelled)
                .map_err(from_gitea)?;
            Ok((client, settings, open))
        });
    match shared {
        Ok((client, settings, open)) => {
            for (input, located) in group {
                let result = lookup(&client, &located, &settings, &open, cancelled);
                batch.found.push((input, result));
            }
        }
        Err(error) => {
            let inputs = group.into_iter().map(|(input, _)| input).collect();
            batch.failed.push((inputs, error));
        }
    }
}

/// The one pull request `located` stands for: an open one from its head, else
/// the newest one from its head into the default branch, which may be merged
/// or closed. A `pr/<number>` fork branch is looked up by its number.
fn lookup(
    client: &Client,
    located: &Located,
    settings: &Repository,
    open: &[GiteaPull],
    cancelled: &impl Fn() -> bool,
) -> Result {
    let repo = &located.repo;
    let pull = |number: u64| -> crate::Result<Option<GiteaPull>> {
        client
            .get_optional(&repo.api(&format!("/pulls/{number}")), cancelled)
            .map_err(from_gitea)
    };
    let found = if let Some(number) = super::fork_branch_number(&located.branch) {
        pull(number)?.filter(|pr| {
            pr.head_owner()
                .is_some_and(|owner| !owner.eq_ignore_ascii_case(&repo.owner))
        })
    } else {
        let mut heads: Vec<&GiteaPull> = open
            .iter()
            .filter(|pr| build::same_head(pr, &located.head_owner, &located.branch))
            .collect();
        match heads.len() {
            // Not among the open ones listed (all of them, or the first
            // `MAX_PAGES` pages of a very busy repository): the newest pull
            // request from this head into the default branch, in any state.
            0 if !settings.default_branch.is_empty()
                && located.head_owner.eq_ignore_ascii_case(&repo.owner) =>
            {
                let path = format!(
                    "/pulls/{}/{}",
                    herdr_gitea::encode(&settings.default_branch),
                    herdr_gitea::encode(&located.branch)
                );
                client
                    .get_optional::<GiteaPull>(&repo.api(&path), cancelled)
                    .map_err(from_gitea)?
                    .filter(|pr| build::same_head(pr, &located.head_owner, &located.branch))
            }
            0 => None,
            1 => pull(heads.remove(0).number)?,
            _ => return Err(Error::PrAmbiguous),
        }
    };
    let Some(pr) = found else {
        return Ok(None);
    };
    let status = if super::parse::object_id(&pr.head.sha) {
        client
            .get_optional(
                &repo.api(&format!("/commits/{}/status", pr.head.sha)),
                cancelled,
            )
            .map_err(from_gitea)?
            .unwrap_or_default()
    } else {
        Default::default()
    };
    let (reviews, _) = client
        .get_pages(
            &repo.api(&format!("/pulls/{}/reviews", pr.number)),
            2,
            cancelled,
        )
        .map_err(from_gitea)?;
    build::pull_request(repo, &pr, settings, &status, &reviews).map(Some)
}

/// Gitea's failure as this crate's, keeping cancellation and timeouts the
/// variants the batch logic already treats specially.
pub fn from_gitea(error: herdr_gitea::Error) -> Error {
    match error {
        herdr_gitea::Error::Cancelled => Error::PrCancelled,
        herdr_gitea::Error::Timeout => Error::PrTimeout,
        error => Error::Gitea(error),
    }
}
