//! Syncing a Gitea source, as the GitLab arm of `sync_source` syncs GitLab's:
//! a complete listing replaces the cache; a capped one, or one a read-only
//! store cannot keep, is shown without being written.

use super::{SyncState, Worker};
use crate::{Access, Result, SourceKey};
use chrono::Utc;
use std::sync::atomic::Ordering;

impl Worker {
    pub(super) fn sync_gitea(&mut self, key: &SourceKey) -> Result<SyncState> {
        let Some(store) = self.store.as_mut() else {
            return Ok(SyncState::Unsupported);
        };
        let writable = store.access() == Access::ReadWrite;
        let previous = store.checkpoint(key)?;
        let cancelled = &self.cancelled;
        let listing = crate::gitea::sync(key, previous.as_ref(), &|| {
            cancelled.load(Ordering::Relaxed)
        })?;
        let count = listing.items.len();
        if !listing.complete || !writable {
            self.uncached.insert(key.clone(), listing.items);
            return Ok(if listing.complete {
                SyncState::Synced {
                    at: Utc::now(),
                    count,
                }
            } else {
                SyncState::Capped { count }
            });
        }
        store.replace_items(key, &listing.items, &listing.checkpoint)?;
        self.uncached.remove(key);
        Ok(SyncState::Synced {
            at: Utc::now(),
            count,
        })
    }
}
