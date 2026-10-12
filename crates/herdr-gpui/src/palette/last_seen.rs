//! The last frame each host streamed for a tab before it moved to another,
//! so Go To can preview a pane that is no longer on screen, labelled with
//! when it was seen. The daemon streams only the tab a client shows, and
//! this client never asks it for more.
//!
//! Frames are the connection's own shared `Arc`s, kept as they arrived:
//! remembering one copies nothing. Each is stamped with its daemon boot and
//! never shown for another, and a host keeps at most [`MAX_TABS`].

use herdr_client::protocol::{ClientShellSnapshot, PaneSurfaceFrame};
use std::{collections::VecDeque, sync::Arc};

/// Tabs remembered per host; the longest unseen go first beyond this.
const MAX_TABS: usize = 16;

struct Seen {
    tab: String,
    surface: Arc<PaneSurfaceFrame>,
    /// Unix seconds when the tab was left.
    at: u64,
}

/// One host's remembered frames, oldest first.
#[derive(Default)]
pub(crate) struct LastSeen {
    frames: VecDeque<Seen>,
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
            self.frames.retain(|seen| seen.surface.boot_id == boot);
        }
        if let (Some(tab), Some(surface)) = (left, before.1)
            && shown != Some(tab)
            // A tab already gone is never chosen, so it is not worth a slot.
            && after.0.is_some_and(|snapshot| snapshot.tabs.iter().any(|t| t.tab_id == tab))
        {
            self.frames.retain(|seen| seen.tab != tab);
            self.frames.push_back(Seen {
                tab: tab.to_owned(),
                surface: surface.clone(),
                at: now,
            });
            if self.frames.len() > MAX_TABS {
                self.frames.pop_front();
            }
        }
        if let Some(tab) = shown {
            self.frames.retain(|seen| seen.tab != tab);
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
