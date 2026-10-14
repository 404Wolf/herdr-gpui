//! Many checkouts' lookups in one GraphQL request. Each checkout is an aliased
//! `repository` field, so a refresh of twenty worktrees is one round trip,
//! and lookups GitHub prices below a point share one instead of costing one
//! each. Every alias is still parsed and verified like a single lookup.

use super::{
    Input, Origin, Origins, Result,
    fetch::{Ask, FIELDS, TIMEOUT, Target, target},
    parse_graphql, parse_numbered,
};
use serde_json::Value;
use std::{fmt::Write as _, time::Duration};

/// The most checkouts one request asks for. A branch lookup reads at most
/// about ten thousand nodes, so twenty stay well inside GitHub's node limit.
pub(super) const BATCH_LIMIT: usize = 20;

const SETTINGS: &str = "mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed";

/// What a batch found: each checkout's own answer, and the checkouts a
/// failure of the whole request (an exhausted quota, the network) left
/// unanswered, which share its error.
#[derive(Default)]
pub(super) struct Batch {
    pub found: Vec<(Input, Result)>,
    pub failed: Option<(Vec<Input>, crate::Error)>,
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
    let mut targets = Vec::with_capacity(inputs.len());
    // A checkout Git cannot identify fails alone; the others are still asked.
    for input in inputs {
        match target(&input, origin, origins, &cancelled) {
            Ok(target) => targets.push((input, target)),
            Err(error) => batch.found.push((input, Err(error))),
        }
    }
    if targets.is_empty() {
        return batch;
    }
    let (query, variables, aliases) = query(&targets);
    match crate::github::graphql_aliases(
        token, &query, variables, &aliases, TIMEOUT, cancelled, cooldown,
    ) {
        Ok(answers) => batch.found.extend(targets.into_iter().zip(answers).map(
            |((input, target), answer)| {
                let result = answer.and_then(|response| parse(response, &target));
                (input, result)
            },
        )),
        Err(error) => {
            batch.failed = Some((targets.into_iter().map(|(input, _)| input).collect(), error));
        }
    }
    batch
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
                // requests from a branch of the same name are filtered out.
                let limit = if head.is_some() { 100 } else { 2 };
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
