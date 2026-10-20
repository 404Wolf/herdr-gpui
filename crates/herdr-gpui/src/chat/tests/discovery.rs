use super::*;
use crate::{chat::discovery, state::LiveState};
use herdr_client::{ConnectTarget, protocol::ClientShellSnapshot};

const ID: &str = "0123456789abcdef0123456789abcdef";

fn tokens(value: &str) -> Vec<(String, String)> {
    vec![
        ("model".into(), "opus".into()),
        (herdr_chat_protocol::TOKEN.into(), value.into()),
    ]
}

fn snapshot(agents: &[(&str, &str)]) -> ClientShellSnapshot {
    let mut snapshot = crate::sidebar::layout_tests::snapshot(1);
    let template = snapshot.agents[0].clone();
    snapshot.agents = agents
        .iter()
        .map(|(pane, value)| {
            let mut agent = template.clone();
            agent.pane_id = (*pane).into();
            agent.tokens = tokens(value);
            agent
        })
        .collect();
    snapshot
}

fn connected(peer: bool) -> LiveState {
    let mut live = LiveState::default();
    live.status = crate::state::ConnectionStatus::Connected;
    live.local_daemon_peer = peer;
    live
}

#[test]
fn a_valid_token_names_the_program() {
    assert_eq!(
        discovery::endpoint(&tokens(ID)).map(|id| id.to_string()),
        Some(ID.into())
    );
}

#[test]
fn an_invalid_token_names_nothing() {
    for value in [
        "",
        "0123",
        "0123456789ABCDEF0123456789ABCDEF",
        "../../../../tmp/evil.sock",
        "0123456789abcdef0123456789abcdeg",
        "/tmp/0123456789abcdef0123456789abcdef",
    ] {
        assert_eq!(discovery::endpoint(&tokens(value)), None, "{value}");
    }
    assert_eq!(discovery::endpoint(&[]), None);
}

#[test]
fn a_local_daemon_lists_each_chat_pane_once() {
    let other = "fedcba9876543210fedcba9876543210";
    let snapshot = snapshot(&[("p1", ID), ("p2", "bad"), ("p1", other), ("p3", other)]);
    let targets = discovery::targets(&snapshot, true);
    let panes: Vec<_> = targets.iter().map(|t| t.pane_id.as_str()).collect();
    assert_eq!(panes, ["p1", "p3"]);
    assert_eq!(targets[0].endpoint.as_str(), ID);
}

#[test]
fn chat_panes_are_bounded() {
    let panes: Vec<String> = (0..MAX_CHATS + 4).map(|n| format!("p{n}")).collect();
    let agents: Vec<_> = panes.iter().map(|pane| (pane.as_str(), ID)).collect();
    assert_eq!(
        discovery::targets(&snapshot(&agents), true).len(),
        MAX_CHATS
    );
}

#[test]
fn a_remote_daemon_offers_no_chat() {
    assert!(discovery::targets(&snapshot(&[("p1", ID)]), false).is_empty());
    let live = connected(true);
    for target in [
        ConnectTarget::Ssh {
            target: "host".into(),
            session: "default".into(),
        },
        ConnectTarget::Wsl {
            distro: "Ubuntu".into(),
            session: "default".into(),
        },
        // A socket by path may be forwarded from another host.
        ConnectTarget::Socket("/tmp/herdr.sock".into()),
    ] {
        assert!(!discovery::local(&target, &live), "{target:?}");
    }
}

#[test]
fn only_an_owned_connected_local_daemon_is_local() {
    assert!(discovery::local(&ConnectTarget::Local, &connected(true)));
    assert!(discovery::local(
        &ConnectTarget::Session {
            name: "work".into(),
            development: false
        },
        &connected(true)
    ));
    // Windows has no peer check; its chat pipe's owner is checked instead.
    assert_eq!(
        discovery::local(&ConnectTarget::Local, &connected(false)),
        cfg!(windows)
    );
    assert!(!discovery::local(
        &ConnectTarget::Local,
        &LiveState::default()
    ));
}
