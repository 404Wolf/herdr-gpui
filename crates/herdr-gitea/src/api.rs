//! The parts of Gitea API v1 responses the GUI reads. Every field a newer or
//! older Gitea (or Forgejo) may omit, or send as `null`, has a default, so a
//! missing field degrades one detail instead of failing a lookup.

use serde::{Deserialize, Deserializer};

/// A field Gitea may send as `null` rather than leave out, such as an empty
/// list of statuses or labels: `null` reads as the type's default.
fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct User {
    #[serde(deserialize_with = "nullable")]
    pub login: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RepositoryRef {
    #[serde(deserialize_with = "nullable")]
    pub name: String,
    #[serde(deserialize_with = "nullable")]
    pub full_name: String,
    pub owner: Option<User>,
}

/// A pull request's head or base.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BranchRef {
    #[serde(rename = "ref")]
    #[serde(deserialize_with = "nullable")]
    pub name: String,
    #[serde(deserialize_with = "nullable")]
    pub sha: String,
    /// `None` once a fork's repository was deleted.
    pub repo: Option<RepositoryRef>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PullRequest {
    #[serde(deserialize_with = "nullable")]
    pub number: u64,
    #[serde(deserialize_with = "nullable")]
    pub title: String,
    /// `open` or `closed`; a merged pull request is closed with `merged`.
    #[serde(deserialize_with = "nullable")]
    pub state: String,
    #[serde(deserialize_with = "nullable")]
    pub draft: bool,
    #[serde(deserialize_with = "nullable")]
    pub merged: bool,
    #[serde(deserialize_with = "nullable")]
    pub mergeable: bool,
    #[serde(deserialize_with = "nullable")]
    pub html_url: String,
    #[serde(deserialize_with = "nullable")]
    pub head: BranchRef,
    #[serde(deserialize_with = "nullable")]
    pub base: BranchRef,
    pub user: Option<User>,
    #[serde(deserialize_with = "nullable")]
    pub updated_at: String,
    /// Absent from listings, present on a single pull request.
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub changed_files: Option<u64>,
    pub requested_reviewers: Option<Vec<User>>,
}

impl PullRequest {
    /// The head repository's owner: the fork's, or the base's for a branch of
    /// the same repository.
    pub fn head_owner(&self) -> Option<&str> {
        self.head
            .repo
            .as_ref()
            .and_then(|repo| repo.owner.as_ref())
            .map(|owner| owner.login.as_str())
    }

    pub fn head_repo(&self) -> Option<&str> {
        self.head.repo.as_ref().map(|repo| repo.name.as_str())
    }

    /// A WIP title prefix marks a draft on Gitea versions without `draft`.
    pub fn is_draft(&self) -> bool {
        self.draft || {
            let title = self.title.trim_start().to_ascii_lowercase();
            ["wip:", "[wip]"]
                .into_iter()
                .any(|prefix| title.starts_with(prefix))
        }
    }
}

/// A commit's combined status. `state` reads `pending` even with no
/// statuses at all, so `total_count` says whether any were reported.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CombinedStatus {
    #[serde(deserialize_with = "nullable")]
    pub state: String,
    #[serde(deserialize_with = "nullable")]
    pub total_count: u64,
    #[serde(deserialize_with = "nullable")]
    pub statuses: Vec<Status>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Status {
    #[serde(deserialize_with = "nullable")]
    pub context: String,
    /// `pending`, `success`, `error`, `failure`, or `warning`.
    #[serde(deserialize_with = "nullable")]
    pub status: String,
    #[serde(deserialize_with = "nullable")]
    pub target_url: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Review {
    #[serde(deserialize_with = "nullable")]
    pub id: u64,
    /// `APPROVED`, `REQUEST_CHANGES`, `COMMENT`, `PENDING`, or `REQUEST_REVIEW`.
    #[serde(deserialize_with = "nullable")]
    pub state: String,
    #[serde(deserialize_with = "nullable")]
    pub dismissed: bool,
    #[serde(deserialize_with = "nullable")]
    pub stale: bool,
    pub user: Option<User>,
    #[serde(deserialize_with = "nullable")]
    pub body: String,
    #[serde(deserialize_with = "nullable")]
    pub submitted_at: String,
    #[serde(deserialize_with = "nullable")]
    pub comments_count: u64,
}

/// An inline comment of a review, on a file.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ReviewComment {
    #[serde(deserialize_with = "nullable")]
    pub id: u64,
    #[serde(deserialize_with = "nullable")]
    pub path: String,
    #[serde(deserialize_with = "nullable")]
    pub body: String,
    pub user: Option<User>,
    #[serde(deserialize_with = "nullable")]
    pub created_at: String,
    /// Set once someone marked the conversation resolved.
    pub resolver: Option<User>,
}

/// A comment on an issue's or pull request's conversation.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Comment {
    #[serde(deserialize_with = "nullable")]
    pub id: u64,
    #[serde(deserialize_with = "nullable")]
    pub body: String,
    pub user: Option<User>,
    #[serde(deserialize_with = "nullable")]
    pub created_at: String,
    #[serde(deserialize_with = "nullable")]
    pub html_url: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Label {
    #[serde(deserialize_with = "nullable")]
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Issue {
    #[serde(deserialize_with = "nullable")]
    pub number: u64,
    #[serde(deserialize_with = "nullable")]
    pub title: String,
    #[serde(deserialize_with = "nullable")]
    pub body: String,
    #[serde(deserialize_with = "nullable")]
    pub state: String,
    #[serde(deserialize_with = "nullable")]
    pub html_url: String,
    pub user: Option<User>,
    #[serde(deserialize_with = "nullable")]
    pub labels: Vec<Label>,
    #[serde(deserialize_with = "nullable")]
    pub comments: u64,
    #[serde(deserialize_with = "nullable")]
    pub created_at: String,
    #[serde(deserialize_with = "nullable")]
    pub updated_at: String,
    /// Set when this issue is a pull request, which `type=issues` excludes.
    pub pull_request: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Repository {
    #[serde(deserialize_with = "nullable")]
    pub full_name: String,
    #[serde(deserialize_with = "nullable")]
    pub html_url: String,
    #[serde(deserialize_with = "nullable")]
    pub default_branch: String,
    #[serde(deserialize_with = "nullable")]
    pub allow_merge_commits: bool,
    #[serde(deserialize_with = "nullable")]
    pub allow_squash_merge: bool,
    #[serde(deserialize_with = "nullable")]
    pub allow_rebase: bool,
    #[serde(deserialize_with = "nullable")]
    pub has_issues: bool,
    #[serde(deserialize_with = "nullable")]
    pub has_pull_requests: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Branch {
    #[serde(deserialize_with = "nullable")]
    pub name: String,
}
