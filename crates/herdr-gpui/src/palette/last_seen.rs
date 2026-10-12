//! The last frame each host streamed for a tab before it moved to another,
//! so Go To can preview a pane that is no longer on screen, labelled with
//! when it was seen. The daemon streams only the tab a client shows, and
//! this client never asks it for more.
//!
//! Frames are the connection's own shared `Arc`s, kept as they arrived:
//! remembering one copies nothing, but it keeps alive a frame that would
//! otherwise be freed, about a megabyte for a full window. So memory, not
//! tab count, bounds them: [`HOST_BYTES`] per host and [`TOTAL_BYTES`]
//! across hosts, the longest unseen going first. Each frame is stamped with
//! its daemon boot and never shown for another.

use herdr_client::protocol::{CellData, ClientShellSnapshot, PaneSurfaceFrame};
use std::{collections::VecDeque, sync::Arc};

/// Bytes of frames one host keeps alive.
pub(super) const HOST_BYTES: usize = 4 << 20;
/// Bytes of frames all hosts together keep alive.
pub(super) const TOTAL_BYTES: usize = 8 << 20;
/// Frames per host whatever their size, so empty ones stay bounded too.
const MAX_TABS: usize = 64;
/// The least a non-empty string's heap allocation takes.
const MIN_ALLOCATION: usize = 16;

struct Seen {
    tab: String,
    surface: Arc<PaneSurfaceFrame>,
    /// Unix seconds when the tab was left.
    at: u64,
    bytes: usize,
}

/// One host's remembered frames, oldest first.
#[derive(Default)]
pub(crate) struct LastSeen {
    frames: VecDeque<Seen>,
    bytes: usize,
}

/// The heap a frame's cells hold, which dominates its size. Counted once,
/// when a tab is left.
fn bytes(surface: &PaneSurfaceFrame) -> usize {
    let cells = &surface.frame.cells;
    let symbols: usize = cells
        .iter()
        .map(|cell| match cell.symbol.capacity() {
            0 => 0,
            capacity => capacity.max(MIN_ALLOCATION),
        })
        .sum();
    cells.capacity() * size_of::<CellData>() + symbols
}

/// The tab `surface` streams, by `snapshot`, when both are of one boot.
fn tab_of<'a>(
    snapshot: Option<&'a ClientShellSnapshot>,
    surface: Option<&Arc<PaneSurfaceFrame>>,
) -> Option<&'a str> {
    let (snapshot, surface) = snapshot.zip(surface)?;
    if snapshot.boot_id != surface.boot_id {
        return None;
    }
    // A surface streams one tab; any of its panes says which.
    let shown = surface.panes.first()?;
    snapshot
        .panes
        .iter()
        .find(|pane| pane.pane_id == shown.pane_id)
        .map(|pane| pane.tab_id.as_str())
}

impl LastSeen {
    /// Called as a host's state moves from `before` to `after`: a tab no
    /// longer streamed is remembered as last seen, and the tab now streamed
    /// is forgotten, since its live cells take over.
    pub(crate) fn observe(
        &mut self,
        before: (Option<&ClientShellSnapshot>, Option<&Arc<PaneSurfaceFrame>>),
        after: (Option<&ClientShellSnapshot>, Option<&Arc<PaneSurfaceFrame>>),
        now: u64,
    ) {
        let left = tab_of(before.0, before.1);
        let shown = tab_of(after.0, after.1);
        if let Some(boot) = after.0.map(|snapshot| snapshot.boot_id.as_str()) {
            self.forget(|seen| seen.surface.boot_id != boot);
        }
        if let (Some(tab), Some(surface)) = (left, before.1)
            && shown != Some(tab)
            // A tab already gone is never chosen, so it is not worth a slot.
            && after.0.is_some_and(|snapshot| snapshot.tabs.iter().any(|t| t.tab_id == tab))
        {
            self.forget(|seen| seen.tab == tab);
            let bytes = bytes(surface);
            // A frame larger than the whole budget is not worth evicting for.
            if bytes <= HOST_BYTES {
                self.frames.push_back(Seen {
                    tab: tab.to_owned(),
                    surface: surface.clone(),
                    at: now,
                    bytes,
                });
                self.bytes += bytes;
            }
            while self.bytes > HOST_BYTES || self.frames.len() > MAX_TABS {
                self.forget_oldest();
            }
        }
        if let Some(tab) = shown {
            self.forget(|seen| seen.tab == tab);
        }
    }

    fn forget(&mut self, mut gone: impl FnMut(&Seen) -> bool) {
        let mut freed = 0;
        self.frames.retain(|seen| {
            let keep = !gone(seen);
            if !keep {
                freed += seen.bytes;
            }
            keep
        });
        self.bytes -= freed;
    }

    fn forget_oldest(&mut self) {
        if let Some(seen) = self.frames.pop_front() {
            self.bytes -= seen.bytes;
        }
    }

    /// Keeps every host's frames within [`TOTAL_BYTES`] together, forgetting
    /// the longest unseen of any host first.
    pub(crate) fn trim<'a>(hosts: impl IntoIterator<Item = &'a mut Self>) {
        let mut hosts: Vec<&mut Self> = hosts.into_iter().collect();
        let mut total: usize = hosts.iter().map(|host| host.bytes).sum();
        while total > TOTAL_BYTES {
            let Some(oldest) = hosts
                .iter_mut()
                .filter_map(|host| Some((host.frames.front()?.at, host)))
                .min_by_key(|(at, _)| *at)
                .map(|(_, host)| host)
            else {
                return;
            };
            let before = oldest.bytes;
            oldest.forget_oldest();
            total -= before - oldest.bytes;
        }
    }

    /// Remembered frames of `boot` with when each was left, newest first.
    pub(super) fn frames<'a>(
        &'a self,
        boot: &'a str,
    ) -> impl Iterator<Item = (&'a Arc<PaneSurfaceFrame>, u64)> {
        self.frames
            .iter()
            .rev()
            .filter(move |seen| seen.surface.boot_id == boot)
            .map(|seen| (&seen.surface, seen.at))
    }
}
