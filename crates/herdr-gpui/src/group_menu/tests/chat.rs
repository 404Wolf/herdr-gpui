use super::*;
use crate::chat::Program;
use herdr_chat_protocol::Agent;

fn chats(actions: &[Action]) -> Vec<Agent> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Chat(agent) => Some(*agent),
            _ => None,
        })
        .collect()
}

/// The window's daemon as this user's own, local one.
fn local(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        view.endpoints[0].connection.target = herdr_client::ConnectTarget::Local;
        view.live.local_daemon_peer = true;
        view.live.status = crate::state::ConnectionStatus::Connected;
    });
}

#[gpui::test]
fn the_menu_offers_a_chat_per_agent_once_herdr_acp_is_found(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    local(&view, cx);
    let group = groups(&view, cx)[0];
    view.update(cx, |view, _| view.chats.program = Program::Missing);
    assert!(chats(&actions(&view, cx, group)).is_empty());
    view.update(cx, |view, _| {
        view.chats.program = Program::Found("/opt/herdr-acp".into());
    });
    let listed = actions(&view, cx, group);
    assert_eq!(chats(&listed), Agent::ALL);
    let first = listed
        .iter()
        .position(|action| matches!(action, Action::Chat(_)))
        .unwrap();
    // After the tabs to open, under a heading of their own.
    assert!(listed[..first].iter().all(|action| action.section() == 0));
    let row = &listed[first];
    assert_eq!(row.label(), Agent::ALL[0].display_name());
    assert_eq!(row.heading(), Some("New Chat"));
    assert_eq!(
        row.icon(),
        Some(crate::icons::AgentIcon::from_identity(Some(Agent::ALL[0].name())).path())
    );
    assert_eq!(child_index(&listed, first), first + 1);
}

#[gpui::test]
fn a_remote_host_offers_no_chat(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    let group = groups(&view, cx)[0];
    view.update(cx, |view, _| {
        view.chats.program = Program::Found("/opt/herdr-acp".into());
        view.endpoints[0].connection.target = herdr_client::ConnectTarget::Ssh {
            target: "host".into(),
            session: "default".into(),
        };
    });
    assert!(chats(&actions(&view, cx, group)).is_empty());
}

#[test]
fn each_headed_section_adds_one_child_before_its_rows() {
    let actions = [
        Action::NewBrowserTab,
        Action::Chat(Agent::Claude),
        Action::Chat(Agent::Codex),
        Action::Port {
            number: 3000,
            process: "node".into(),
            link: Link::Page(crate::browser::WebUrl::try_from("http://localhost:3000/").unwrap()),
        },
        Action::Split,
    ];
    assert_eq!(child_index(&actions, 0), 0);
    assert_eq!(child_index(&actions, 1), 2);
    assert_eq!(child_index(&actions, 2), 3);
    assert_eq!(child_index(&actions, 3), 5);
    assert_eq!(child_index(&actions, 4), 6);
}
