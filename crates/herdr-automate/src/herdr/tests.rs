use super::*;
use crate::testing::Fixture;

#[test]
fn worktrees_and_agents_go_through_the_cli() {
    let fixture = Fixture::new();
    let herdr = &fixture.herdr;
    assert!(herdr.running());
    let created = herdr
        .create_worktree(Path::new("/repo"), "nightly/2026-10-22", Some("main"))
        .unwrap();
    assert_eq!(created.pane, "w1p1");
    assert!(created.path.ends_with("worktree"));
    herdr
        .start_agent("nightly-1", "claude", &created.pane)
        .unwrap();
    herdr
        .prompt(&created.pane, "Review main.", Some(Duration::from_secs(60)))
        .unwrap();
    let calls = fixture.calls();
    assert!(
        calls.contains(
            "worktree create --cwd /repo --branch nightly/2026-10-22 --no-focus --base main"
        ),
        "{calls}"
    );
    assert!(
        calls.contains("agent start nightly-1 --kind claude --pane w1p1 --timeout 90000"),
        "{calls}"
    );
    assert!(
        calls.contains("agent prompt w1p1 Review main. --wait --until done --timeout 60000"),
        "{calls}"
    );
}

#[test]
fn herdr_errors_are_typed() {
    let fixture = Fixture::new();
    let error = fixture.herdr.call(&["bogus", "thing"]).unwrap_err();
    match error {
        Error::Failed { command, message } => {
            assert_eq!(command, "bogus thing");
            assert_eq!(message, "unknown_command: not faked");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn pane_details_parse() {
    let fixture = Fixture::new();
    let info = fixture.herdr.process_info("w1p1").unwrap();
    assert_eq!(info.shell_pid, Some(1));
    assert!(!info.at_prompt());
    let pane = fixture.herdr.pane_agent("w1p1").unwrap();
    assert_eq!(pane.agent.as_deref(), Some("claude"));
    assert_eq!(pane.agent_status.as_deref(), Some("working"));
}

#[test]
fn a_missing_binary_is_not_running() {
    let herdr = Herdr::new(Some(PathBuf::from("/nonexistent/herdr")), None);
    assert!(!herdr.running());
    assert!(matches!(herdr.notify("t", "b"), Err(Error::Launch { .. })));
}
