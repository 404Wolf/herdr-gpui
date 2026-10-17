//! Redacted GitHub parser diagnostics retain the original error as a source.
use super::Error;

impl Error {
    pub(crate) fn github_json(source: serde_json::Error) -> Self {
        Self::GitHubJson(GitHubJsonError(source))
    }
}

/// Parser diagnostics can quote credential-bearing fields; expose the cause only
/// to explicit source inspection, never ordinary Display or Debug formatting.
#[derive(thiserror::Error)]
#[error("Invalid GitHub JSON response.")]
pub struct GitHubJsonError(#[source] serde_json::Error);

impl std::fmt::Debug for GitHubJsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GitHubJsonError([REDACTED])")
    }
}

/// Why `[github] accounts`, or a change to it, was refused.
#[derive(Debug, thiserror::Error)]
pub enum GitHubAccountsError {
    #[error("[github] accounts may list at most {0} accounts")]
    TooMany(usize),
    #[error("[github] accounts id must be 32 lowercase hexadecimal digits")]
    Id,
    #[error("[github] accounts lists the id {0} twice")]
    DuplicateId(String),
    #[error("[github] accounts must be a list of account tables")]
    NotAList,
    #[error("[github] accounts owner {0:?} is not a GitHub user or organization name")]
    Owner(String),
    #[error("Could not generate a GitHub account ID")]
    Random(#[source] getrandom::Error),
}
