use super::*;
use crate::{orchestrator::RunStatus, sidebar::activity::counts::Counts};
use std::collections::HashSet;

#[test]
fn counts_active_runs_open_pull_requests_and_open_issues_once() {
    let mut closed = item("3", "Closed issue");
    closed.state = "closed".into();
    let mut pull = item("pr/4", "Open pull request");
    pull.pull_request = Some(pull_request(4));
    let mut merged = item("pr/5", "Merged pull request");
    merged.pull_request = Some(pull_request(5));
    merged.state = "merged".into();
    let items = [
        item("1", "Open"),
        item("2", "Also open"),
        closed,
        pull,
        merged,
    ];
    let runs = [
        run("a", &items[0], RunState::Running),
        run("b", &items[1], RunState::Completed),
        run("c", &items[1], RunState::Running),
    ];
    // Live statuses: the first asks the user, the third's agent went idle.
    let statuses = [RunStatus::NeedsInput, RunStatus::Done, RunStatus::Idle];
    let mut counts = Counts::default();
    let mut seen = HashSet::new();
    counts.add(&mut seen, &items, runs.iter().zip(statuses));
    // The same repository in a second workspace's view counts once.
    counts.add(&mut seen, &items, runs.iter().zip(statuses));
    assert_eq!(counts.of(ActivityPanel::Orchestrator), Some(1));
    assert_eq!(counts.of(ActivityPanel::PullRequests), Some(1));
    assert_eq!(counts.of(ActivityPanel::Issues), Some(2));
    assert_eq!(counts.of(ActivityPanel::Workspaces), None);
}
