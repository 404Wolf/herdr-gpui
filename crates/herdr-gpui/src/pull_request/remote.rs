//! Which GitHub repository a checkout's pull requests belong to, and the Git
//! remote that reaches it. A fork checkout names the base repository with an
//! `upstream` remote and pushes to its own `origin`.

use super::fetch::{git, origin_repository};
use crate::Error;
use std::time::Instant;

/// A Git remote this client reads pull request refs from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Remote {
    #[default]
    Origin,
    Upstream,
}

impl Remote {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Origin => "origin",
            Self::Upstream => "upstream",
        }
    }
}

/// The repository a checkout's pull requests are opened against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Base {
    pub owner: String,
    pub repo: String,
    pub remote: Remote,
    /// For an `upstream` base, the owner of the GitHub fork behind `origin`,
    /// where this checkout's own branches are pushed. `None` when `origin` is
    /// not on GitHub.com, or when the base is `origin` itself.
    pub fork_owner: Option<String>,
}

impl Base {
    /// The `headRefName` a new pull request from `branch`, pushed to
    /// `origin`, names: qualified with the fork's owner when it crosses
    /// repositories, as GitHub requires.
    pub(crate) fn head(&self, branch: &str) -> crate::Result<String> {
        match self.remote {
            Remote::Origin => Ok(branch.to_owned()),
            Remote::Upstream => {
                let fork = self.fork_owner.as_deref().ok_or(Error::PrOrigin)?;
                Ok(format!("{fork}:{branch}"))
            }
        }
    }

    /// Whether a pull request from `branch` would ask to merge a branch into
    /// itself. Across a fork the same name is two different branches.
    pub(crate) fn is_own_base(&self, branch: &str, default_branch: &str) -> bool {
        self.remote == Remote::Origin && branch == default_branch
    }
}

/// A GitHub `upstream` remote when the checkout has one, otherwise origin.
/// An `upstream` that is not on GitHub.com is ignored rather than fatal, so
/// such checkouts keep working exactly as before.
pub(crate) fn base_repository(
    checkout: &str,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> crate::Result<Base> {
    if let Ok(upstream) = git(
        checkout,
        &["config", "--default", "", "--get", "remote.upstream.url"],
        deadline,
        cancelled,
    ) && let Some((owner, repo)) = crate::avatars::github_repo(&upstream)
    {
        let fork_owner = origin_repository(checkout, deadline, cancelled)
            .ok()
            .map(|(fork, _)| fork)
            .filter(|fork| !fork.eq_ignore_ascii_case(&owner));
        return Ok(Base {
            owner,
            repo,
            remote: Remote::Upstream,
            fork_owner,
        });
    }
    let (owner, repo) = origin_repository(checkout, deadline, cancelled)?;
    Ok(Base {
        owner,
        repo,
        remote: Remote::Origin,
        fork_owner: None,
    })
}
