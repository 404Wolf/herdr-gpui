//! A Gitea repository's open pull requests and issues for the picker. Gitea's
//! REST answers are put in the shape `model::parse` reads, so a Gitea row is
//! cleaned, bounded, and checked exactly like a GitHub one, then given the
//! repository's own web addresses.

use super::{Item, Kind, Origin, model::LIMIT, model::parse};
use crate::{
    Error,
    pull_request::{Input, Remote, gitea_from_error, local_gitea_repository},
};
use herdr_gitea::{Client, Issue, PullRequest, Repo};
use serde_json::{Value, json};
use std::time::Instant;

#[cfg(test)]
#[path = "gitea_tests.rs"]
mod tests;

/// The listing for `input` when its repository is on a configured Gitea
/// host, `None` when it is GitHub's to list.
pub(super) fn list(
    input: &Input,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> Option<crate::Result<(Origin, Vec<Item>)>> {
    let (repo, fork_owner) = match local_gitea_repository(input, deadline, cancelled) {
        Ok(Some(found)) => found,
        Ok(None) => return None,
        Err(error) => return Some(Err(error.into())),
    };
    Some((|| {
        let timeout = deadline
            .checked_duration_since(Instant::now())
            .ok_or(Error::PrTimeout)?;
        let client = Client::new(&repo.host, timeout).map_err(gitea_from_error)?;
        // One page each: the tabs are a picker, not a mirror of the repository.
        let (pulls, _): (Vec<PullRequest>, bool) = client
            .get_pages(
                &repo.api("/pulls?state=open&sort=recentupdate"),
                1,
                cancelled,
            )
            .map_err(gitea_from_error)?;
        let (issues, _): (Vec<Issue>, bool) = client
            .get_pages(&repo.api("/issues?type=issues&state=open"), 1, cancelled)
            .map_err(gitea_from_error)?;
        let origin = Origin {
            owner: repo.owner.clone(),
            repo: repo.name.clone(),
            remote: if fork_owner.is_some() {
                Remote::Upstream
            } else {
                Remote::Origin
            },
            fork_owner,
        };
        Ok((origin.clone(), items(&repo, &origin, &pulls, &issues)?))
    })())
}

/// The rows a listing shows, with Gitea's web addresses.
pub(super) fn items(
    repo: &Repo,
    origin: &Origin,
    pulls: &[PullRequest],
    issues: &[Issue],
) -> crate::Result<Vec<Item>> {
    let response = shaped(pulls, issues);
    let mut items = parse(&response, origin, Kind::PullRequest)?;
    items.extend(parse(&response, origin, Kind::Issue)?);
    for item in &mut items {
        item.url = format!(
            "{}/{}/{}",
            repo.web(),
            match item.kind {
                Kind::PullRequest => "pulls",
                Kind::Issue => "issues",
            },
            item.number
        );
    }
    Ok(items)
}

/// Gitea's answers as the GraphQL response `parse` reads.
fn shaped(pulls: &[PullRequest], issues: &[Issue]) -> Value {
    let login = |user: &Option<herdr_gitea::User>| {
        user.as_ref()
            .map(|user| user.login.clone())
            .unwrap_or_default()
    };
    let pulls: Vec<Value> = pulls
        .iter()
        .take(LIMIT)
        .map(|pr| {
            json!({
                "number": pr.number,
                "title": pr.title,
                "isDraft": pr.is_draft(),
                "headRefName": pr.head.name,
                "author": {"login": login(&pr.user)},
                "headRepositoryOwner": {"login": pr.head_owner().unwrap_or_default()},
            })
        })
        .collect();
    let issues: Vec<Value> = issues
        .iter()
        .filter(|issue| issue.pull_request.is_none())
        .take(LIMIT)
        .map(|issue| {
            json!({
                "number": issue.number,
                "title": issue.title,
                "author": {"login": login(&issue.user)},
            })
        })
        .collect();
    json!({"data": {"repository": {
        "pullRequests": {"nodes": pulls},
        "issues": {"nodes": issues},
    }}})
}
