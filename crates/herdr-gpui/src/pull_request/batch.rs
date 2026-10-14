//! Many checkouts' lookups in one GraphQL request. Each checkout is an aliased
//! `repository` field, so a refresh of twenty worktrees is one round trip,
//! and lookups GitHub prices below a point share one instead of costing one
//! each. Every alias is still parsed and verified like a single lookup.

use super::{
    Input, Origin, Origins, Result,
    fetch::{Ask, FIELDS, Head, NUMBER_QUERY, TIMEOUT, Target, target},
    parse_graphql, parse_numbered,
};
use crate::Error;
use serde_json::{Value, json};
use std::{
    fmt::Write as _,
    time::{Duration, Instant},
};

/// The most checkouts one request asks for. A branch lookup reads at most a
/// few hundred nodes, so twenty stay well inside GitHub's node limit.
pub(super) const BATCH_LIMIT: usize = 20;

/// Pull requests read per branch with an upstream. GitHub prices a lookup by
/// how many it may return, and few branch names have more across forks; one
/// that does is looked up again (see [`crowded`]).
pub(super) const HEAD_LIMIT: u64 = 5;

/// How long a batch spends identifying its checkouts before it asks GitHub,
/// so one slow SSH device cannot hold every other checkout's answer back.
const PREPARE: Duration = TIMEOUT;

const SETTINGS: &str = "mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed";

/// Only the heads of a crowded branch name's pull requests: cheap to list.
const HEADS: &str = r#"query($owner: String!, $repo: String!, $branch: String!) {
  repository(owner: $owner, name: $repo) {
    pullRequests(first: 100, headRefName: $branch, orderBy: {field: UPDATED_AT, direction: DESC}) {
      pageInfo { hasNextPage }
      nodes { number headRepositoryOwner { login } headRepository { name } }
    }
  }
}"#;

/// What a batch found: each checkout's own answer; the checkouts a failure of
/// a whole request (an exhausted quota, the network) left unanswered, which
/// share its error; and those it never asked about, which stay due.
#[derive(Default)]
pub(super) struct Batch {
    pub found: Vec<(Input, Result)>,
    pub failed: Vec<(Vec<Input>, Error)>,
    pub deferred: Vec<Input>,
}

pub(super) fn fetch_batch(
    inputs: Vec<Input>,
    origin: &Origin,
    origins: &mut Origins,
    token: &secrecy::SecretString,
    cancelled: impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> Batch {
    let mut batch = Batch::default();
    let targets = prepare(
        inputs,
        Instant::now() + PREPARE,
        Instant::now,
        |input, deadline| target(input, origin, origins, deadline, &cancelled),
        &mut batch,
    );
    ask(
        targets,
        |query, variables, aliases, cooldown| {
            crate::github::graphql_aliases(
                token, query, variables, aliases, TIMEOUT, &cancelled, cooldown,
            )
        },
        |response, target, cooldown| answer_for(response, target, token, &cancelled, cooldown),
        (&mut batch, cooldown),
    );
    batch
}

/// Identifies each checkout until `deadline`. A checkout Git cannot identify
/// fails alone; one the deadline leaves unprepared is deferred, not failed.
pub(super) fn prepare(
    inputs: Vec<Input>,
    deadline: Instant,
    now: impl Fn() -> Instant,
    mut target: impl FnMut(&Input, Instant) -> crate::Result<Target>,
    batch: &mut Batch,
) -> Vec<(Input, Target)> {
    let mut targets = Vec::with_capacity(inputs.len());
    for input in inputs {
        if now() >= deadline {
            batch.deferred.push(input);
            continue;
        }
        match target(&input, deadline) {
            Ok(target) => targets.push((input, target)),
            Err(error) => batch.found.push((input, Err(error))),
        }
    }
    targets
}

/// Asks about every target. Each response is bounded as one lookup's was;
/// many busy checkouts together may not fit, so a group too large is halved
/// and asked again. After any other refusal the rest wait for a later batch,
/// which a pause that refusal set holds back.
pub(super) fn ask(
    targets: Vec<(Input, Target)>,
    mut request: impl FnMut(
        &str,
        Value,
        &[(String, String)],
        &mut Option<Duration>,
    ) -> crate::Result<Vec<crate::Result<Value>>>,
    mut answer: impl FnMut(Value, &Target, &mut Option<Duration>) -> Result,
    (batch, cooldown): (&mut Batch, &mut Option<Duration>),
) {
    let mut groups = vec![targets];
    while let Some(mut group) = groups.pop() {
        if group.is_empty() {
            continue;
        }
        let (query, variables, aliases) = query(&group);
        match request(&query, variables, &aliases, cooldown) {
            Ok(answers) => {
                for ((input, target), response) in group.into_iter().zip(answers) {
                    let result = response.and_then(|response| answer(response, &target, cooldown));
                    batch.found.push((input, result));
                }
            }
            Err(Error::GitHubSize) if group.len() > 1 => {
                let second = group.split_off(group.len() / 2);
                groups.push(second);
                groups.push(group);
            }
            Err(error) => {
                batch
                    .failed
                    .push((group.into_iter().map(|(input, _)| input).collect(), error));
                batch
                    .deferred
                    .extend(groups.drain(..).flatten().map(|(input, _)| input));
            }
        }
    }
}

/// One alias's pull request, looked up again when its branch name is crowded.
fn answer_for(
    response: Value,
    target: &Target,
    token: &secrecy::SecretString,
    cancelled: &impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> Result {
    if let Ask::Branch {
        branch,
        head: Some(head),
    } = &target.ask
        && response["data"]["repository"]["pullRequests"]["pageInfo"]["hasNextPage"] == true
    {
        return crowded(target, branch, head, token, cancelled, cooldown);
    }
    parse(response, target)
}

/// A branch name more than [`HEAD_LIMIT`] pull requests share across forks:
/// list only their heads, then read the one the upstream names in full. Two
/// cheap requests, and only for such a branch.
fn crowded(
    target: &Target,
    branch: &str,
    head: &Head,
    token: &secrecy::SecretString,
    cancelled: &impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> Result {
    let (owner, repo) = (target.owner.as_str(), target.repo.as_str());
    let heads = crate::github::graphql(
        "pull_request",
        token,
        HEADS,
        json!({"owner": owner, "repo": repo, "branch": branch}),
        TIMEOUT,
        cancelled,
        cooldown,
    )?;
    let number = matching_number(&heads, head)?;
    let Some(number) = number else {
        return Ok(None);
    };
    let mut response = crate::github::graphql(
        "pull_request",
        token,
        NUMBER_QUERY,
        json!({"owner": owner, "repo": repo, "number": number}),
        TIMEOUT,
        cancelled,
        cooldown,
    )?;
    let pr = response["data"]["repository"]["pullRequest"].take();
    if pr.is_null() {
        return Ok(None);
    }
    response["data"]["repository"]["pullRequests"] = json!({"nodes": [pr]});
    parse_graphql(response, owner, repo, branch, Some(head))
}

/// The one listed pull request from `head`, if there is one.
pub(super) fn matching_number(heads: &Value, head: &Head) -> crate::Result<Option<u64>> {
    let listing = &heads["data"]["repository"]["pullRequests"];
    if listing["pageInfo"]["hasNextPage"] == true {
        return Err(Error::PrAmbiguous);
    }
    let numbers: Vec<u64> = listing["nodes"]
        .as_array()
        .ok_or(Error::PrRepository)?
        .iter()
        .filter(|pr| {
            pr["headRepositoryOwner"]["login"]
                .as_str()
                .is_some_and(|owner| owner.eq_ignore_ascii_case(&head.owner))
                && pr["headRepository"]["name"]
                    .as_str()
                    .is_some_and(|repo| repo.eq_ignore_ascii_case(&head.repo))
        })
        .filter_map(|pr| pr["number"].as_u64())
        .collect();
    match numbers.as_slice() {
        [] => Ok(None),
        [number] => Ok(Some(*number)),
        _ => Err(Error::PrAmbiguous),
    }
}

pub(super) fn parse(response: Value, target: &Target) -> Result {
    match &target.ask {
        Ask::Branch { branch, head } => {
            parse_graphql(response, &target.owner, &target.repo, branch, head.as_ref())
        }
        Ask::Number(number) => parse_numbered(response, &target.owner, &target.repo, *number),
    }
}

/// The query, its variables, and each alias with the `owner/repo` it reads.
/// Names and branches travel only as variables, never in the query text.
pub(super) fn query(targets: &[(Input, Target)]) -> (String, Value, Vec<(String, String)>) {
    let mut declarations = Vec::with_capacity(targets.len());
    let mut fields = String::new();
    let mut variables = serde_json::Map::new();
    let mut aliases = Vec::with_capacity(targets.len());
    for (index, (_, target)) in targets.iter().enumerate() {
        variables.insert(format!("o{index}"), target.owner.clone().into());
        variables.insert(format!("r{index}"), target.repo.clone().into());
        let lookup = match &target.ask {
            Ask::Branch { branch, head } => {
                declarations.push(format!(
                    "$o{index}: String!, $r{index}: String!, $b{index}: String!, $l{index}: Int!"
                ));
                variables.insert(format!("b{index}"), branch.clone().into());
                // An upstream head names its owner, so other forks' pull
                // requests from a branch of the same name are filtered out;
                // without one, two are enough to tell an ambiguous branch.
                let limit = if head.is_some() { HEAD_LIMIT } else { 2 };
                variables.insert(format!("l{index}"), limit.into());
                format!(
                    "pullRequests(first: $l{index}, headRefName: $b{index}, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ pageInfo {{ hasNextPage }} nodes {{ ...PullRequestFields }} }}"
                )
            }
            Ask::Number(number) => {
                declarations.push(format!(
                    "$o{index}: String!, $r{index}: String!, $n{index}: Int!"
                ));
                variables.insert(format!("n{index}"), (*number).into());
                format!("pullRequest(number: $n{index}) {{ ...PullRequestFields }}")
            }
        };
        let _ = writeln!(
            fields,
            "  p{index}: repository(owner: $o{index}, name: $r{index}) {{ {SETTINGS} {lookup} }}"
        );
        aliases.push((
            format!("p{index}"),
            format!("{}/{}", target.owner, target.repo),
        ));
    }
    let query = format!("query({}) {{\n{fields}}}{FIELDS}", declarations.join(", "));
    (query, Value::Object(variables), aliases)
}
