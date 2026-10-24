//! A Gitea pull request, its commit status, and its reviews as the
//! `PullRequest` the badges, menus, and actions already read. The value is
//! assembled in the shape GitHub's GraphQL answers deserialize from, then held
//! to the same identity checks and cleaning a GitHub lookup gets.

use super::{
    super::{MergeMethod, PullRequest, clean, parse::object_id},
    ID_PREFIX,
};
use crate::Error;
use herdr_gitea::{CombinedStatus, PullRequest as GiteaPull, Repo, Repository, Review};
use serde_json::{Value, json};

/// Whether `pr` is from `branch` as pushed under `owner`. A head whose fork
/// was deleted has no owner to check and matches only by name.
pub(super) fn same_head(pr: &GiteaPull, owner: &str, branch: &str) -> bool {
    pr.head.name == branch
        && pr
            .head_owner()
            .is_none_or(|head| head.eq_ignore_ascii_case(owner))
}

pub(super) fn pull_request(
    repo: &Repo,
    pr: &GiteaPull,
    settings: &Repository,
    status: &CombinedStatus,
    reviews: &[Review],
) -> crate::Result<PullRequest> {
    // Rebuilt from the repository that was asked for, so an answer can never
    // point a badge or a merge at another repository.
    let expected = format!("{}/pulls/{}", repo.web(), pr.number);
    if pr.number == 0 || !pr.html_url.eq_ignore_ascii_case(&expected) {
        return Err(Error::PrIdentity);
    }
    let state = match (pr.state.as_str(), pr.merged) {
        (_, true) => "MERGED",
        ("open", false) => "OPEN",
        ("closed", false) => "CLOSED",
        _ => return Err(Error::PrIdentity),
    };
    let head_owner = pr.head_owner().unwrap_or(&repo.owner).to_owned();
    let value = json!({
        "number": pr.number,
        "url": expected,
        "title": pr.title,
        "state": state,
        "isDraft": pr.is_draft(),
        "headRefName": pr.head.name,
        "baseRefName": pr.base.name,
        "additions": pr.additions.unwrap_or_default(),
        "deletions": pr.deletions.unwrap_or_default(),
        "changedFiles": pr.changed_files.unwrap_or_default(),
        "updatedAt": pr.updated_at,
        "mergeStateStatus": merge_state(pr),
        "reviewDecision": review_decision(pr, reviews),
        "headRepositoryOwner": {"login": head_owner},
        "id": format!("{ID_PREFIX}{}", pr.number),
        "headRefOid": if object_id(&pr.head.sha) { pr.head.sha.as_str() } else { "" },
        "statusCheckRollup": checks(status),
    });
    let mut pr: PullRequest = serde_json::from_value(value).map_err(Error::github_json)?;
    pr.title = clean(&pr.title);
    pr.head_ref_name = clean(&pr.head_ref_name);
    pr.base_ref_name = clean(&pr.base_ref_name);
    pr.updated_at = clean(&pr.updated_at);
    for check in pr.status_check_rollup.iter_mut().flatten() {
        check.name = clean(&check.name).chars().take(128).collect();
    }
    pr.checks_summary = pr.checks();
    pr.merge_methods = [
        (MergeMethod::Merge, settings.allow_merge_commits),
        (MergeMethod::Squash, settings.allow_squash_merge),
        (MergeMethod::Rebase, settings.allow_rebase),
    ]
    .into_iter()
    .filter_map(|(method, allowed)| allowed.then_some(method))
    .collect();
    Ok(pr)
}

/// Gitea reports only whether an open pull request merges cleanly, so the
/// finer GitHub states (behind, blocked) never appear.
pub(super) fn merge_state(pr: &GiteaPull) -> &'static str {
    if pr.state != "open" || pr.merged {
        "UNKNOWN"
    } else if pr.is_draft() {
        "DRAFT"
    } else if pr.mergeable {
        "CLEAN"
    } else {
        "DIRTY"
    }
}

/// Each reported status as a GitHub status context. `warning` does not fail
/// a commit on Gitea, so it reads as passed.
pub(super) fn checks(status: &CombinedStatus) -> Value {
    Value::Array(
        status
            .statuses
            .iter()
            .take(100)
            .map(|status| {
                let state = match status.status.as_str() {
                    "success" | "warning" => "SUCCESS",
                    "failure" => "FAILURE",
                    "error" => "ERROR",
                    "skipped" => "SKIPPED",
                    _ => "PENDING",
                };
                json!({"__typename": "StatusContext", "context": status.context, "state": state})
            })
            .collect(),
    )
}

/// GitHub's review decision from Gitea's reviews: each reviewer's latest
/// verdict counts, requested changes outweigh approvals, and with neither a
/// pending review request still asks for one. Dismissed reviews never count;
/// comments and drafts do not replace an earlier verdict.
pub(super) fn review_decision(pr: &GiteaPull, reviews: &[Review]) -> Option<&'static str> {
    let mut latest: Vec<(&str, &str)> = Vec::new();
    for review in reviews {
        let verdict = match review.state.as_str() {
            "APPROVED" | "REQUEST_CHANGES" if !review.dismissed => review.state.as_str(),
            _ => continue,
        };
        let Some(user) = review.user.as_ref().map(|user| user.login.as_str()) else {
            continue;
        };
        // Gitea lists reviews oldest first, so a later one replaces the earlier.
        match latest.iter_mut().find(|(reviewer, _)| *reviewer == user) {
            Some(entry) => entry.1 = verdict,
            None => latest.push((user, verdict)),
        }
    }
    if latest
        .iter()
        .any(|(_, verdict)| *verdict == "REQUEST_CHANGES")
    {
        Some("CHANGES_REQUESTED")
    } else if latest.iter().any(|(_, verdict)| *verdict == "APPROVED") {
        Some("APPROVED")
    } else if pr
        .requested_reviewers
        .as_ref()
        .is_some_and(|reviewers| !reviewers.is_empty())
    {
        Some("REVIEW_REQUIRED")
    } else {
        None
    }
}
