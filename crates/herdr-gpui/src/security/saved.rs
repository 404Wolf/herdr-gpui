//! The last read, kept in the app's cache (`crate::cache`) under the logins
//! it was read as, so a restart lists alerts and reports at once while the
//! first read of the run is still out. Only what the list shows is kept:
//! names, alerts, and report titles and metadata, never a report's
//! description, which is not read for the list at all, and never a token.
//! A repository whose read failed keeps what the cache last held for it.
//!
//! Until the run's own read lands, a restored repository has no account to
//! read an item's page with, and no checkouts to start an agent in.

use super::{Alerts, Repo, Reports};
use crate::cache::{Cache, Kind};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// One repository as the cache keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Saved {
    name: String,
    host: String,
    alerts: Alerts,
    reports: Reports,
}

impl Saved {
    fn of(repo: &Repo) -> Option<Self> {
        Some(Self {
            name: repo.name.clone(),
            host: repo.host.clone(),
            alerts: repo.alerts.as_ref().ok()?.clone(),
            reports: repo.reports.as_ref().ok()?.clone(),
        })
    }

    fn into_repo(self) -> Repo {
        Repo {
            name: self.name,
            host: self.host,
            alerts: Ok(self.alerts),
            reports: Ok(self.reports),
            account: None,
            checkouts: Vec::new(),
        }
    }
}

/// The cache's key for a read as `accounts`, sorted.
pub(super) fn key(accounts: &[String]) -> String {
    accounts.join("\n")
}

/// What the cache holds for `accounts`, and when it was read.
pub(super) fn restore(cache: &Cache, accounts: &[String]) -> Option<(Vec<Repo>, SystemTime)> {
    let entry = cache
        .get::<Vec<Saved>>(Kind::Security, &key(accounts))
        .inspect_err(|error| tracing::debug!(%error, "Cannot read the cached Security read"))
        .ok()??;
    let repos = entry.value.into_iter().map(Saved::into_repo).collect();
    Some((repos, entry.saved_at.into()))
}

/// Keeps `repos` for `accounts`. A repository whose read failed keeps what
/// was kept for it before; one no longer read is dropped.
pub(super) fn save(cache: &Cache, accounts: &[String], repos: &[Repo]) {
    let key = key(accounts);
    let before = cache
        .get::<Vec<Saved>>(Kind::Security, &key)
        .ok()
        .flatten()
        .map(|entry| entry.value)
        .unwrap_or_default();
    let saved: Vec<Saved> = repos
        .iter()
        .filter_map(|repo| {
            Saved::of(repo).or_else(|| before.iter().find(|kept| kept.name == repo.name).cloned())
        })
        .collect();
    if let Err(error) = cache.put(Kind::Security, &key, &saved) {
        tracing::debug!(%error, "Cannot cache the Security read");
    }
}
