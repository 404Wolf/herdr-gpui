//! Notes the user sent that no terminal took, on a page or on the agent's
//! changes: kept for the agent to fetch with `browser feedback`. A batch goes one way only. When its agent is
//! waiting in `browser feedback --wait`, the batch is handed there; otherwise
//! it is pasted into the agent's pane, and only a pane that no longer exists
//! leaves it here. Only the Unix control socket fetches or waits, so the
//! methods for it exist only where that socket does.
use super::Scope;
use gpui::Global;
use std::{collections::VecDeque, sync::Arc};

/// Batches kept at once; the oldest goes first.
const MAX_KEPT: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FeedbackKey {
    pub scope: Scope,
    pub pane_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Batch {
    /// The Herdr pane of the agent the notes are for.
    pub target: FeedbackKey,
    pub text: String,
}

#[derive(Default)]
pub(crate) struct Feedback {
    /// Each batch with its pending marker and its number in `added` order.
    kept: VecDeque<(Batch, Option<Arc<()>>, u64)>,
    /// Batches kept so far, which numbers the next one.
    added: u64,
    /// Panes whose agents are waiting in `browser feedback --wait`.
    waiting: Vec<FeedbackKey>,
}

impl Global for Feedback {}

impl Feedback {
    #[cfg(any(unix, test))]
    pub(crate) fn waiting(&self) -> &[FeedbackKey] {
        &self.waiting
    }

    pub(crate) fn is_waiting(&self, target: &FeedbackKey) -> bool {
        self.waiting.contains(target)
    }

    /// Records which panes are waiting.
    #[cfg(any(unix, test))]
    pub(crate) fn set_waiting(&mut self, panes: Vec<FeedbackKey>) {
        self.waiting = panes;
    }

    pub(crate) fn keep(&mut self, batch: Batch, pending: Option<Arc<()>>) {
        if self.kept.len() >= MAX_KEPT {
            self.kept.pop_front();
        }
        self.added += 1;
        self.kept.push_back((batch, pending, self.added));
    }

    /// Everything kept for this daemon and pane, oldest first, joined into one text.
    #[cfg(any(unix, test))]
    pub(crate) fn take(&mut self, target: &FeedbackKey) -> Option<String> {
        let (taken, kept): (Vec<_>, Vec<_>) = self
            .kept
            .drain(..)
            .partition(|(batch, ..)| &batch.target == target);
        self.kept = kept.into();
        (!taken.is_empty()).then(|| {
            taken
                .into_iter()
                .map(|(batch, ..)| batch.text)
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    /// Whom the kept batches are for, oldest first, each with its number:
    /// numbers only grow, so a reader tells new batches from old ones even
    /// once the oldest are dropped.
    #[cfg(unix)]
    pub(crate) fn kept(&self) -> impl Iterator<Item = (u64, &FeedbackKey)> {
        self.kept
            .iter()
            .map(|(batch, _, added)| (*added, &batch.target))
    }

    #[cfg(any(unix, test))]
    pub(crate) fn has(&self, target: &FeedbackKey) -> bool {
        self.kept.iter().any(|(batch, ..)| &batch.target == target)
    }
}

#[cfg(test)]
mod tests;
