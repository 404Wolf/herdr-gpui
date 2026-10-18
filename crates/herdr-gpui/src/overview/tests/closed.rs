use super::*;
use crate::{
    overview::{closed::Closed, reopen},
    sidebar::layout_tests::snapshot,
    teleport::AgentKind,
};
use herdr_client::protocol::{ClientShellPane, ClientShellSnapshot};

/// A report of host `local` under `boot`, with a pane for each agent.
fn report(boot: &str, agents: &[&str]) -> (ClientShellSnapshot, Vec<Row>) {
    let mut shown = snapshot(1);
    shown.boot_id = boot.into();
    shown.panes = agents
        .iter()
        .map(|agent| ClientShellPane {
            pane_id: format!("pane-{agent}"),
            workspace_id: "w0".into(),
            tab_id: "t0".into(),
            label: None,
            cwd: Some("/src/herdr".into()),
            foreground_cwd: Some(format!("/src/herdr/{agent}")),
            focused: false,
            right_click_passthrough: false,
        })
        .collect();
    let rows = agents
        .iter()
        .map(|agent| row(agent, State::Working))
        .collect();
    (shown, rows)
}

fn observe(closed: &mut Closed, boot: &str, agents: &[&str], now: u64) {
    let (shown, rows) = report(boot, agents);
    closed.observe("local", &shown, &rows, at(now));
}

fn names(closed: &Closed) -> Vec<String> {
    closed
        .agents()
        .map(|agent| agent.row.detail.clone())
        .collect()
}

#[test]
fn an_agent_gone_from_its_host_is_closed_with_its_folder() {
    let mut closed = Closed::default();
    observe(&mut closed, "b1", &["a", "b"], 10);
    // The first report closes nothing: there was nothing before it.
    assert!(closed.is_empty());
    let revision = closed.revision();
    observe(&mut closed, "b1", &["b"], 20);
    assert_eq!(names(&closed), ["a"]);
    assert!(closed.revision() > revision);
    let agent = closed.get(&key("a")).unwrap();
    assert_eq!(agent.closed, at(20));
    assert_eq!(agent.cwd.as_deref(), Some("/src/herdr/a"));
    // Newest first.
    observe(&mut closed, "b1", &[], 30);
    assert_eq!(names(&closed), ["b", "a"]);
}

#[test]
fn a_daemon_restart_closes_nothing() {
    let mut closed = Closed::default();
    observe(&mut closed, "b1", &["a"], 10);
    observe(&mut closed, "b2", &[], 20);
    assert!(closed.is_empty());
}

#[test]
fn an_agent_that_comes_back_is_no_longer_closed() {
    let mut closed = Closed::default();
    observe(&mut closed, "b1", &["a"], 10);
    observe(&mut closed, "b1", &[], 20);
    observe(&mut closed, "b1", &["a"], 30);
    assert!(closed.is_empty());
}

#[test]
fn a_twin_still_running_keeps_its_closed_namesake_listed() {
    let mut closed = Closed::default();
    let (mut shown, mut rows) = report("b1", &["a"]);
    let mut twin = rows[0].clone();
    if let Open::Pane { pane, .. } = &mut twin.open {
        *pane = "pane-twin".into();
    }
    rows.push(twin.clone());
    closed.observe("local", &shown, &rows, at(10));
    shown.panes.truncate(0);
    closed.observe("local", &shown, [&twin], at(20));
    closed.observe("local", &shown, [&twin], at(30));
    assert_eq!(names(&closed), ["a"]);
}

#[test]
fn forgetting_and_dropped_hosts_clear_closed_agents() {
    let mut closed = Closed::default();
    observe(&mut closed, "b1", &["a", "b"], 10);
    observe(&mut closed, "b1", &[], 20);
    closed.forget(&key("a"));
    assert_eq!(names(&closed), ["b"]);
    closed.retain_hosts(|id| id != "local");
    assert!(closed.is_empty());
}

#[test]
fn closed_agents_are_bounded() {
    let mut closed = Closed::default();
    let agents: Vec<String> = (0..40).map(|index| format!("agent-{index}")).collect();
    let all: Vec<&str> = agents.iter().map(String::as_str).collect();
    observe(&mut closed, "b1", &all, 10);
    observe(&mut closed, "b1", &[], 20);
    assert!(closed.len() < all.len());
    assert!(!closed.is_empty());
}

#[test]
fn a_reopened_agent_continues_where_it_can() {
    assert_eq!(reopen::command_line(AgentKind::Claude), "claude --continue");
    assert_eq!(
        reopen::command_line(AgentKind::Codex),
        "codex resume --last"
    );
    assert_eq!(
        reopen::command_line(AgentKind::Opencode),
        "opencode --continue"
    );
    // An agent without a known way to continue starts afresh.
    assert_eq!(reopen::command_line(AgentKind::Cursor), "cursor-agent");
}

#[test]
fn only_known_agents_can_be_reopened() {
    let mut closed = Closed::default();
    observe(&mut closed, "b1", &["a"], 10);
    observe(&mut closed, "b1", &[], 20);
    let agent = closed.get(&key("a")).unwrap().clone();
    assert_eq!(reopen::kind(&agent), Some(AgentKind::Claude));
    let mut unknown = agent;
    unknown.row.identity = Some("make".into());
    assert_eq!(reopen::kind(&unknown), None);
}
