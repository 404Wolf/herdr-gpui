//! The bar's count bubbles, from the orchestrator views the window holds:
//! runs an agent is on or that wait on the user, open pull requests, and
//! open issues. An item or run two views list, such as one repository open
//! in two workspaces, counts once.

use super::ActivityPanel;
use crate::{
    HerdrWindow,
    orchestrator::{Item, Run, RunStatus},
};
use gpui::App;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Counts {
    runs: usize,
    pull_requests: usize,
    issues: usize,
}

impl Counts {
    /// One view's lists added to the counts, skipping what `seen` holds.
    pub(super) fn add<'a>(
        &mut self,
        seen: &mut HashSet<String>,
        items: &'a [Item],
        runs: impl IntoIterator<Item = (&'a Run, RunStatus)>,
    ) {
        for (run, status) in runs {
            if active(status) && seen.insert(format!("run:{}", run.id)) {
                self.runs += 1;
            }
        }
        for item in items {
            if !open(item) || !seen.insert(item.key.canonical()) {
                continue;
            }
            if item.pull_request.is_some() {
                self.pull_requests += 1;
            } else {
                self.issues += 1;
            }
        }
    }

    /// The bar's count for `panel`, if it shows one.
    pub(super) fn of(self, panel: ActivityPanel) -> Option<usize> {
        match panel {
            ActivityPanel::Workspaces => None,
            ActivityPanel::Orchestrator => Some(self.runs),
            ActivityPanel::PullRequests => Some(self.pull_requests),
            ActivityPanel::Issues => Some(self.issues),
        }
    }
}

/// Whether an agent is on the run or it waits on the user.
fn active(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Starting | RunStatus::Working | RunStatus::NeedsInput | RunStatus::Failed
    )
}

/// Whether the provider still counts the item as open. Beads' own word for
/// finished work counts as closed too.
fn open(item: &Item) -> bool {
    !matches!(item.state.as_str(), "closed" | "merged" | "done")
}

impl HerdrWindow {
    pub(super) fn activity_counts(&self, cx: &App) -> Counts {
        let mut views = self.orchestrator_views();
        views.sort_unstable_by_key(|(tab, _)| *tab);
        let mut counts = Counts::default();
        let mut seen = HashSet::new();
        for (_, view) in &views {
            let view = view.read(cx);
            let statuses = view.run_statuses();
            counts.add(&mut seen, view.items(), view.runs().iter().zip(statuses));
        }
        counts
    }
}
