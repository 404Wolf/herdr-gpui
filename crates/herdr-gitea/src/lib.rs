//! Gitea and Forgejo hosts, as the GUI reaches them: the `[gitea]` config
//! table, which remotes belong to a configured host, the token each host is
//! read with, and a bounded REST client for API v1.
//!
//! Nothing here knows about GitHub. The pull request prefetch, its actions,
//! the repository picker, and the orchestrator each translate what this crate
//! reads into their own models, so the GitHub code paths stay as they are.
//! Every request blocks: call it from a worker thread, never the UI thread.

mod api;
pub mod config;
mod http;
mod registry;
mod remote;
mod tls;
mod token;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

#[cfg(test)]
mod tests;

pub use api::{
    Branch, BranchRef, CombinedStatus, Comment, Issue, Label, PullRequest, Repository,
    RepositoryRef, Review, ReviewComment, Status, User,
};
pub use config::{Config, HostConfig};
pub use http::{Client, MAX_PAGES, PAGE};
pub use registry::{Host, configure, configured, hosts};
pub use remote::{Repo, encode, host_of, pull_url, repo, repo_in};
pub use token::token;

/// Why a Gitea host could not be read.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Gitea could not be reached: {0}")]
    Network(#[source] Box<ureq::Error>),
    #[error("Gitea's answer could not be read: {0}")]
    Read(#[source] std::io::Error),
    #[error("Gitea's answer exceeded the size limit")]
    Size,
    #[error("Gitea's answer is not the expected JSON: {0}")]
    Json(#[source] serde_json::Error),
    #[error("Gitea's address {0:?} is not valid")]
    Address(String),
    #[error("The Gitea token for {0} is not one an HTTP header can carry")]
    Token(String),
    #[error("The Gitea token command for {host} failed: {reason}")]
    TokenCommand { host: String, reason: String },
    #[error("Gitea refused the token for {0}; check the host's token_command")]
    Authentication(String),
    #[error("Gitea refused access to {0}")]
    Forbidden(String),
    #[error("Gitea shows nothing at {0} to this token")]
    NotFound(String),
    #[error("Gitea could not do that: {0}")]
    Refused(String),
    #[error("Gitea rate limit reached; it is read again later")]
    RateLimit,
    #[error("Gitea request failed (HTTP {0})")]
    Status(u16),
    #[error("Gitea request timed out")]
    Timeout,
    #[error("Gitea request cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;
