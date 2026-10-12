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
/// Text one `take` hands out, measured as JSON, so the answer that carries
/// it fits the control socket's message bound. Batches past it stay kept for
/// the next take.
#[cfg(any(unix, test))]
pub(crate) const TAKE_BUDGET: usize = 240 * 1024;

/// `text`'s size inside a JSON string.
#[cfg(any(unix, test))]
pub(crate) fn json_size(text: &str) -> usize {
    serde_json::to_string(text).map_or(text.len() * 6, |json| json.len())
}

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

    /// What is kept for this daemon and pane, oldest first, joined into one
    /// text of at most [`TAKE_BUDGET`] (always at least one batch); the rest
    /// stays for the next take.
    #[cfg(any(unix, test))]
    pub(crate) fn take(&mut self, target: &FeedbackKey) -> Option<String> {
        let mut used = 0;
        let mut taken = Vec::new();
        let mut rest = VecDeque::with_capacity(self.kept.len());
        // Oldest first and in order: once one batch does not fit, every
        // later one for this pane waits too, so a newer note never arrives
        // before an older one.
        let mut full = false;
        for entry in self.kept.drain(..) {
            if !full && &entry.0.target == target {
                let size = json_size(&entry.0.text) + 1;
                if taken.is_empty() || used + size <= TAKE_BUDGET {
                    used += size;
                    taken.push(entry);
                    continue;
                }
                full = true;
            }
            rest.push_back(entry);
        }
        self.kept = rest;
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
