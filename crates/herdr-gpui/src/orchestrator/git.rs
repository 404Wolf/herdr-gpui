//! Repositories on this machine, read with gix rather than by starting `git`:
//! what `repo::detect`, the agent branch listing, and the base commit need.
//! Removing a worktree still runs `git`, which owns that mutation.

use super::{Error, Result};
use std::path::{Path, PathBuf};

/// What `repo::detect`'s script prints for a checkout.
pub(super) struct Checkout {
    /// The git common directory, canonicalized as `pwd -P` resolves it.
    pub(super) common: PathBuf,
    /// The main checkout, else this one when the main repository is bare.
    pub(super) main: PathBuf,
    /// Every remote and its fetch URL with `insteadOf` rewrites applied, as
    /// `git remote get-url` prints it; empty when it has none.
    pub(super) remotes: Vec<(String, String)>,
}

fn git(operation: &'static str) -> impl FnOnce(Box<dyn std::error::Error + Send + Sync>) -> Error {
    move |source| Error::Git { operation, source }
}

/// The repository holding `checkout`, or `None` when it is not in one.
pub(super) fn checkout(checkout: &Path) -> Result<Option<Checkout>> {
    const READING: &str = "Reading the repository";
    use gix::discover::upwards::Error as Upwards;
    // A missing folder or one outside Git is "not a repository", as `git
    // rev-parse` failing was; anything else is a failure worth showing.
    if !checkout.is_dir() {
        return Ok(None);
    }
    let repo = match gix::discover(checkout) {
        Ok(repo) => repo,
        Err(error)
            if matches!(
                error.downcast_any_ref::<Upwards>(),
                Some(
                    Upwards::NoGitRepository { .. }
                        | Upwards::NoGitRepositoryWithinCeiling { .. }
                        | Upwards::NoGitRepositoryWithinFs { .. }
                )
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(git(READING)(error.into())),
    };
    let Some(root) = repo.workdir().map(Path::to_path_buf) else {
        // A bare repository has no checkout to list issues for.
        return Ok(None);
    };
    let common = super::repo::plain(
        repo.common_dir()
            .canonicalize()
            .map_err(|error| git(READING)(error.into()))?,
    );
    let main_repo = repo
        .main_repo()
        .map_err(|error| git(READING)(error.into()))?;
    let main = main_repo.workdir().map_or(root, Path::to_path_buf);
    let remotes = main_repo
        .remote_names()
        .into_iter()
        .map(|name| {
            let url = main_repo
                .find_remote(name.as_slice())
                .ok()
                .and_then(|remote| {
                    remote
                        .url(gix::remote::Direction::Fetch)
                        .map(|url| url.to_bstring().to_string())
                })
                .unwrap_or_default();
            (name.to_string(), url)
        })
        .collect();
    Ok(Some(Checkout {
        common,
        main,
        remotes,
    }))
}

/// Up to `max` branch names under `refs/heads/{prefix}`, short, in name
/// order, as `git for-each-ref --format=%(refname:short)` lists them.
pub(super) fn branches(main_root: &Path, prefix: &str, max: usize) -> Result<Vec<String>> {
    const READING: &str = "Reading the branches";
    let repo = gix::open(main_root).map_err(|error| git(READING)(error.into()))?;
    let references = repo
        .references()
        .map_err(|error| git(READING)(error.into()))?;
    let full = format!("refs/heads/{prefix}");
    let branches = references
        .prefixed(full.as_str())
        .map_err(|error| git(READING)(error.into()))?
        .filter_map(Result::ok)
        .map(|reference| reference.name().shorten().to_string())
        .take(max)
        .collect();
    Ok(branches)
}

/// The full commit id `base` names in `main_root`: a reference such as
/// `HEAD` or a branch, peeled through tags, else a full commit id.
pub(super) fn commit(main_root: &Path, base: &str) -> Result<String> {
    const READING: &str = "Reading the base commit";
    let repo = gix::open(main_root).map_err(|error| git(READING)(error.into()))?;
    let found = match repo
        .try_find_reference(base)
        .map_err(|error| git(READING)(error.into()))?
    {
        Some(mut reference) => {
            reference
                .peel_to_commit()
                .map_err(|error| git(READING)(error.into()))?
                .id
        }
        None => {
            let id = gix::ObjectId::from_hex(base.as_bytes())
                .map_err(|error| git(READING)(error.into()))?;
            repo.find_object(id)
                .map_err(|error| git(READING)(error.into()))?
                .peel_to_commit()
                .map_err(|error| git(READING)(error.into()))?
                .id
        }
    };
    Ok(found.to_string())
}
