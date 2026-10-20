#![allow(clippy::unwrap_used)]
use super::*;
use std::cell::Cell;

fn agent(terminal: &str) -> Agent {
    Agent {
        pane_id: "p1".into(),
        terminal_id: terminal.into(),
        directory: PathBuf::from("fixture-claude-store"),
        runtime: None,
        agent: Some("claude".into()),
        agent_status: herdr_client::protocol::AgentStatus::Idle,
        agent_session: Some(AgentSession {
            agent: "claude".into(),
            kind: SessionKind::Id,
            source: "herdr:claude".into(),
            value: "00000000-0000-4000-8000-000000000001".into(),
        }),
    }
}

#[test]
fn a_replacement_between_screen_read_and_submit_never_receives_input() {
    let expected = identity(&agent("old"), "p1").unwrap();
    let calls = Cell::new(0);
    let sent = Cell::new(false);
    let result = checked_send(
        "p1",
        &expected,
        "hello",
        || {
            calls.set(calls.get() + 1);
            Ok(agent(if calls.get() == 1 { "old" } else { "new" }))
        },
        || Ok("────────────\n❯\n────────────".into()),
        || {
            sent.set(true);
            Ok(())
        },
        || true,
    );
    assert!(matches!(result, Err(Error::TargetChanged)));
    assert!(!sent.get());
}

#[test]
fn cancelled_and_busy_sends_are_refused_and_success_is_submitted_once() {
    let expected = identity(&agent("t1"), "p1").unwrap();
    let sent = Cell::new(0);
    for connected in [false, true] {
        let result = checked_send(
            "p1",
            &expected,
            "hello",
            || Ok(agent("t1")),
            || Ok("────────────\n❯\n────────────".into()),
            || {
                sent.set(sent.get() + 1);
                Ok(())
            },
            || connected,
        );
        assert_eq!(result.is_ok(), connected);
    }
    assert_eq!(sent.get(), 1);
    let result = checked_send(
        "p1",
        &expected,
        "hello",
        || {
            let mut current = agent("t1");
            current.agent_status = herdr_client::protocol::AgentStatus::Blocked;
            Ok(current)
        },
        || panic!("must not read"),
        || panic!("must not send"),
        || true,
    );
    assert!(matches!(result, Err(Error::NotReady)));
}

#[test]
fn only_a_proven_empty_box_accepts_chat_input() {
    assert!(empty_input(
        "answer\n────────────\n❯ \n\n────────────\n? shortcuts"
    ));
    for body in ["❯ a draft", "❯\ncontinued", "!", "❯ Try a suggested prompt"] {
        assert!(!empty_input(&format!("────────────\n{body}\n────────────")));
    }
    assert!(!empty_input("❯"));
    assert!(!empty_input("────────────\n❯"));
}

#[test]
fn prompts_preserve_newlines_but_reject_terminal_controls() {
    assert!(validate_prompt("first\nsecond\tline").is_ok());
    for text in ["", "  ", "hello\u{1b}[31m", "hello\rworld", "hello\u{7}"] {
        assert!(matches!(validate_prompt(text), Err(Error::InvalidPrompt)));
    }
    assert!(matches!(
        validate_prompt(&"a".repeat(chat::MAX_PROMPT_BYTES + 1)),
        Err(Error::InvalidPrompt)
    ));
}

#[test]
fn reused_process_identity_is_not_allowed_to_receive_an_old_draft() {
    let mut initial = agent("t1");
    initial.runtime = Some(super::super::session::Runtime {
        pid: 42,
        started: "original birth".into(),
    });
    let expected = identity(&initial, "p1").unwrap();
    let calls = Cell::new(0);
    let result = checked_send(
        "p1",
        &expected,
        "hello",
        || {
            calls.set(calls.get() + 1);
            let mut current = agent("t1");
            current.runtime = Some(super::super::session::Runtime {
                pid: 42,
                started: if calls.get() == 1 {
                    "original birth"
                } else {
                    "reused pid"
                }
                .into(),
            });
            Ok(current)
        },
        || Ok("────────────\n❯\n────────────".into()),
        || panic!("must not send"),
        || true,
    );
    assert!(matches!(result, Err(Error::TargetChanged)));
}
