//! Reading the alerts of every repository the window's workspaces hold, on a
//! background thread: each checkout's remote is read as the orchestrator
//! reads it, then per distinct github.com repository one GraphQL request for
//! its alerts and up to two REST requests for its private reports.

use super::{
    Error,
    model::{Alerts, Repo, parse, valid_name},
};
use crate::{github::Tokens, orchestrator::RepoSpec};
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(20);

/// Open alerts per repository. The section is a triage list, not a mirror:
/// GitHub's own page holds the rest, and the count says how many there are.
pub(super) const QUERY: &str = r#"query($owner: String!, $repo: String!) {
  repository(owner: $owner, name: $repo) {
    hasVulnerabilityAlertsEnabled
    vulnerabilityAlerts(first: 100, states: OPEN) {
      totalCount
      nodes {
        number createdAt vulnerableManifestPath
        securityVulnerability {
          severity vulnerableVersionRange
          firstPatchedVersion { identifier }
          package { name ecosystem }
          advisory { ghsaId summary }
        }
      }
    }
  }
}"#;

/// The alerts of each distinct github.com repository `specs` hold, in their
/// order. A checkout on another provider, or one whose remote cannot be
/// read, is left out; the orchestrator's own sections report those.
pub(super) fn read(specs: &[RepoSpec], cancelled: &AtomicBool) -> Vec<Repo> {
    let mut seen = HashSet::new();
    let mut repos = Vec::new();
    // Once the quota is spent, the rest would only be refused too.
    let mut limited: Option<Arc<Error>> = None;
    for spec in specs {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let Some(tokens) = &spec.request.token else {
            continue;
        };
        let name = match crate::orchestrator::github_repository(
            &spec.request.target,
            &spec.request.checkout,
            cancelled,
        ) {
            Ok(Some(name)) if valid_name(&name) => name,
            Ok(_) => continue,
            Err(error) => {
                tracing::debug!(category = "security", %error, "Cannot read a checkout's remote");
                continue;
            }
        };
        if !seen.insert(name.to_ascii_lowercase()) {
            continue;
        }
        let alerts = match &limited {
            Some(error) => Err(error.clone()),
            None => alerts(&name, tokens, cancelled).map_err(Arc::new),
        };
        note_limit(&alerts, &mut limited);
        let reports = match &limited {
            Some(error) => Err(error.clone()),
            None => super::reports::read(&name, tokens, || cancelled.load(Ordering::Relaxed))
                .map_err(Arc::new),
        };
        note_limit(&reports, &mut limited);
        repos.push(Repo {
            name,
            host: spec.host.clone(),
            alerts,
            reports,
        });
    }
    repos
}

/// Remembers a spent quota, so the round's remaining reads are skipped.
fn note_limit<T>(read: &Result<T, Arc<Error>>, limited: &mut Option<Arc<Error>>) {
    if let Err(error) = read
        && error.rate_limited()
    {
        *limited = Some(error.clone());
    }
}

fn alerts(name: &str, tokens: &Tokens, cancelled: &AtomicBool) -> Result<Alerts, Error> {
    let (owner, repo) = name.split_once('/').ok_or(Error::Output)?;
    // Each repository's refusal is its own: a token that may not read one
    // repository's alerts must not pause the others, as the shared cooldown
    // would. An exhausted quota stops the round through `limited` instead.
    let mut cooldown = None;
    let response = crate::github::graphql(
        "security",
        tokens.for_owner(owner),
        QUERY,
        serde_json::json!({"owner": owner, "repo": repo}),
        TIMEOUT,
        || cancelled.load(Ordering::Relaxed),
        &mut cooldown,
    )
    .map_err(|error| Error::Github(Box::new(error)))?;
    parse(&response)
}
