use super::*;
use crate::{config::AgentsPlacement, state::ConnectionStatus};

/// Two hosts, each with two workspaces and an agent in each of them, so rows
/// carry their `workspace-…` and `agent-…` selectors.
fn two_hosts(window: &mut Window, cx: &mut Context<HerdrWindow>) -> HerdrWindow {
    let mut view = fixture_window(window, cx);
    let listing = snapshot(2);
    view.live.snapshot = Some(Arc::new(listing.clone()));
    view.live.status = ConnectionStatus::Connected;
    let mut remote = crate::endpoint::Endpoint::new(
        "ssh:test".into(),
        "Remote".into(),
        ConnectTarget::Ssh {
            target: "unused".into(),
            session: "default".into(),
        },
        true,
    );
    remote.live.snapshot = Some(Arc::new(listing));
    remote.live.status = ConnectionStatus::Connected;
    view.endpoints.push(remote);
    view
}

/// Per host: its two workspace rows, then the agent row of each.
const HOSTS: [[&str; 4]; 2] = [
    [
        "workspace-local-w0",
        "workspace-local-w1",
        "agent-local-p0",
        "agent-local-p1",
    ],
    [
        "workspace-ssh:test-w0",
        "workspace-ssh:test-w1",
        "agent-ssh:test-p0",
        "agent-ssh:test-p1",
    ],
];

fn show(
    view: &Entity<HerdrWindow>,
    (show_agents, placement): (bool, AgentsPlacement),
    cx: &mut gpui::VisualTestContext,
) {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.config.show_agents = show_agents;
            view.config.agents_placement = placement;
            cx.notify();
        });
        full_draw(window, cx).clear(cx);
    });
}

#[gpui::test]
fn nested_agents_sit_under_their_workspace_and_drop_the_agents_section(
    cx: &mut gpui::TestAppContext,
) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| two_hosts(window, cx));
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    cx.simulate_resize(size(px(800.), px(800.)));

    show(&view, (true, AgentsPlacement::Nested), cx);
    assert!(cx.debug_bounds("agents-section").is_none());
    for [first, second, under_first, under_second] in HOSTS {
        let host = first;
        let [first, second, under_first, under_second] =
            [first, second, under_first, under_second].map(|row| cx.debug_bounds(row).unwrap());
        assert!(first.bottom() <= under_first.top(), "{host}");
        assert!(under_first.bottom() <= second.top(), "{host}");
        assert!(second.bottom() <= under_second.top(), "{host}");
        // A step in from the workspace, and inside the sidebar.
        assert!(under_first.left() >= first.left(), "{host}");
        assert!(under_first.right() <= cx.debug_bounds("sidebar").unwrap().right());
    }
    // The spaces list takes the height the agents section had.
    assert_eq!(
        cx.debug_bounds("spaces-section").unwrap().size.height,
        cx.debug_bounds("sidebar").unwrap().size.height
            - cx.debug_bounds("device-footer").unwrap().size.height
    );

    show(&view, (true, AgentsPlacement::Panel), cx);
    let agents = cx.debug_bounds("agents-section").unwrap();
    for [_, _, agent, _] in HOSTS {
        assert!(
            agents.contains(&cx.debug_bounds(agent).unwrap().center()),
            "{agent}"
        );
    }

    // Hidden agents stay hidden wherever they would be placed.
    show(&view, (false, AgentsPlacement::Nested), cx);
    assert!(cx.debug_bounds("agents-section").is_none());
    assert!(cx.debug_bounds("agent-local-p0").is_none());
    assert!(cx.debug_bounds("workspace-local-w0").is_some());
}
