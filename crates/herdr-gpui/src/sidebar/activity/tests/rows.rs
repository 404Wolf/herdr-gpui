use super::*;
use crate::orchestrator::RunStatus;
use crate::sidebar::activity::rows::{
    self, BranchPr, Opens, ROW_LIMIT, Section, Source, Tag, Tone, filter,
};

fn source<'a>(items: &'a [Item], runs: &'a [Run], statuses: &'a [RunStatus]) -> Source<'a> {
    Source {
        host: tab(),
        items,
        runs,
        statuses,
    }
}

/// The statuses runs have with no live agent: their stored states.
fn stored(runs: &[Run]) -> Vec<RunStatus> {
    runs.iter().map(|run| RunStatus::of(run, None)).collect()
}

fn titles(section: &Section) -> Vec<&str> {
    section.rows.iter().map(|row| row.title.as_str()).collect()
}

#[test]
fn runs_are_grouped_by_what_they_need_from_the_user() {
    let items = [item("1", "Asks"), item("2", "Works"), item("3", "Done")];
    let runs = [
        run("a", &items[2], RunState::Completed),
        run("b", &items[1], RunState::Running),
        run("c", &items[0], RunState::NeedsInput),
    ];
    let sections = rows::orchestrator(&[source(&items, &runs, &stored(&runs))]);
    let names: Vec<_> = sections.iter().map(|section| section.title).collect();
    assert_eq!(names, ["Needs you", "Working", "Idle", "Finished"]);
    assert_eq!(titles(&sections[0]), ["Asks"]);
    assert_eq!(titles(&sections[1]), ["Works"]);
    assert!(sections[2].rows.is_empty());
    assert_eq!(titles(&sections[3]), ["Done"]);
    let asking = &sections[0].rows[0];
    assert_eq!(asking.number, "#1");
    assert_eq!(asking.meta, "herdr-gpui · claude");
    assert_eq!(
        asking.tag,
        Some(Tag {
            label: "needs input",
            tone: Tone::Attention
        })
    );
    assert_eq!(
        asking.opens,
        Opens::Run {
            host: tab(),
            id: "c".into()
        }
    );
    // Asking and working runs count; finished ones do not.
    assert_eq!(
        rows::active_runs(&[source(&items, &runs, &stored(&runs))]),
        2
    );
}

#[test]
fn a_run_whose_item_is_gone_is_named_by_its_agent() {
    let gone = item("9", "Gone");
    let runs = [run("a", &gone, RunState::Running)];
    let sections = rows::orchestrator(&[source(&[], &runs, &stored(&runs))]);
    let row = &sections[1].rows[0];
    assert_eq!((row.number.as_str(), row.title.as_str()), ("", "claude"));
}

#[test]
fn issues_list_open_items_and_mark_those_an_agent_works_on() {
    let mut closed = item("3", "Closed");
    closed.state = "closed".into();
    let mut pull = item("pr/4", "A pull request");
    pull.pull_request = Some(pull_request(4));
    let items = [item("1", "Worked on"), item("2", "Waiting"), closed, pull];
    let runs = [
        run("a", &items[0], RunState::Running),
        run("b", &items[1], RunState::Completed),
    ];
    let sections = rows::issues(&[source(&items, &runs, &stored(&runs))]);
    assert_eq!(titles(&sections[0]), ["Worked on", "Waiting"]);
    assert_eq!(sections[0].rows[0].tag.map(|tag| tag.label), Some("agent"));
    assert_eq!(sections[0].rows[1].tag, None);
    assert_eq!(sections[0].rows[1].meta, "herdr-gpui · enhancement");
    assert_eq!(
        sections[0].rows[0].opens,
        Opens::Item {
            host: tab(),
            key: items[0].key.canonical()
        }
    );
}

#[test]
fn branch_pull_requests_come_first_and_are_not_listed_twice() {
    let mut pr = crate::pull_request::fixture().unwrap();
    pr.url = "https://github.com/penso/herdr-gpui/pull/8".into();
    // Two worktrees on the branch, and the orchestrator lists it too.
    let branches = [
        BranchPr {
            workspace: "w1",
            pr: &pr,
        },
        BranchPr {
            workspace: "w2",
            pr: &pr,
        },
    ];
    let mut listed = item("pr/8", "Listed again");
    listed.pull_request = Some(pull_request(8));
    let mut other = item("pr/9", "Only in the orchestrator");
    other.pull_request = Some(pull_request(9));
    let items = [listed, other];
    let sections = rows::pull_requests(&branches, &[source(&items, &[], &stored(&[]))]);
    assert_eq!(sections[0].rows.len(), 1);
    let yours = &sections[0].rows[0];
    assert_eq!(yours.number, "#8");
    assert_eq!(yours.meta, "herdr-gpui · feature");
    assert_eq!(yours.opens, Opens::Workspace("w1".into()));
    // The fixture's checks include a failure, which outranks the rest.
    assert_eq!(yours.tag.map(|tag| tag.label), Some("CI failed"));
    assert_eq!(titles(&sections[1]), ["Only in the orchestrator"]);
}

#[test]
fn a_settled_or_draft_pull_request_says_so_before_its_checks() {
    let mut pr = crate::pull_request::fixture().unwrap();
    let tag = |pr: &crate::pull_request::PullRequest| {
        rows::pull_requests(&[BranchPr { workspace: "w", pr }], &[])[0].rows[0]
            .tag
            .map(|tag| tag.label)
    };
    pr.is_draft = true;
    assert_eq!(tag(&pr), Some("draft"));
    pr.state = crate::pull_request::State::Merged;
    assert_eq!(tag(&pr), Some("merged"));
}

#[test]
fn filtering_matches_any_text_and_drops_emptied_sections() {
    let items = [item("12", "Sidebar navigation"), item("13", "Tray icon")];
    let sections = rows::issues(&[source(&items, &[], &stored(&[]))]);
    assert_eq!(
        titles(&filter(sections.clone(), "  SIDEBAR ")[0]),
        ["Sidebar navigation"]
    );
    assert_eq!(titles(&filter(sections.clone(), "#13")[0]), ["Tray icon"]);
    assert_eq!(filter(sections.clone(), "").len(), 1);
    assert!(filter(sections, "nothing like it").is_empty());
}

#[test]
fn every_section_is_bounded() {
    let items: Vec<_> = (0..ROW_LIMIT + 5)
        .map(|n| item(&n.to_string(), "Issue"))
        .collect();
    assert_eq!(
        rows::issues(&[source(&items, &[], &stored(&[]))])[0]
            .rows
            .len(),
        ROW_LIMIT
    );
}

#[test]
fn a_live_agent_status_outranks_the_stored_state() {
    let items = [item("1", "Stored as running"), item("2", "Stored as done")];
    let runs = [
        run("a", &items[0], RunState::Running),
        run("b", &items[1], RunState::Completed),
    ];
    // The view's statuses: the first run's agent now waits on the user, the
    // second's went back to work.
    let live = [RunStatus::NeedsInput, RunStatus::Working];
    let sources = [source(&items, &runs, &live)];
    let sections = rows::orchestrator(&sources);
    assert_eq!(titles(&sections[0]), ["Stored as running"]);
    assert_eq!(titles(&sections[1]), ["Stored as done"]);
    assert_eq!(rows::active_runs(&sources), 2);
    let issues = rows::issues(&sources);
    assert!(issues[0].rows.iter().all(|row| row.tag.is_some()));
}
