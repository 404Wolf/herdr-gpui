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
