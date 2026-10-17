//! The orchestrator the panels read: their own until a tab opens, and a
//! tab's on another device than the one shown.
use crate::{
    browser::{Location, OrchestratorRepo, Store, scope},
    sidebar::layout_tests::{fixture_window, snapshot},
    sidebar::{ActivityPanel, activity::Host},
    state::ConnectionStatus,
};
use herdr_client::ConnectTarget;
use std::sync::Arc;

#[gpui::test]
fn a_row_from_another_device_switches_to_it(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        let mut listing = snapshot(2);
        listing.focused_workspace_id = Some("w1".into());
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
    });
    // The orchestrator is opened on this machine's workspace w1.
    let id = cx.update(|_, cx| {
        let tab_scope = scope(&view.read(cx).endpoints[0]);
        Store::update(cx, |store| {
            store.open(
                tab_scope,
                "w1",
                Some(Location::Orchestrator {
                    repo: OrchestratorRepo::new("/nonexistent/activity-test".into()).unwrap(),
                }),
                None,
            )
        })
        .unwrap()
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.poll_orchestrators(window, cx);
            assert!(view.orchestrators.contains_key(&id));
            // Then the remote device is shown.
            assert!(view.select_endpoint("ssh:test", cx));
            assert_eq!(view.selected_endpoint, 1);
            assert!(view.show_orchestrator_tab(id, window, cx).is_some());
            // Its row brings back the device the orchestrator is on.
            assert_eq!(view.selected_endpoint, 0);
        })
    });
}

#[gpui::test]
fn a_list_panel_connects_the_orchestrator_and_its_tab_takes_the_view(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        // The local session can run the orchestrator's scripts.
        view.endpoints[0].connection.target = ConnectTarget::Local;
        let mut listing = snapshot(2);
        listing.focused_workspace_id = Some("w0".into());
        view.live.snapshot = Some(Arc::new(listing));
        view.live.status = ConnectionStatus::Connected;
        view
    });
    cx.update(|window, cx| {
        // The app's tab store, which the window keeps its tabs in.
        Store::update(cx, |_| ());
        view.update(cx, |view, cx| {
            // The Workspaces panel alone reads nothing.
            view.poll_orchestrators(window, cx);
            assert!(view.activity.orchestrator.is_none());
            view.show_activity_panel(ActivityPanel::Issues, cx);
            view.poll_orchestrators(window, cx);
            let connected = view.activity.orchestrator.as_ref().unwrap();
            assert_eq!(connected.workspace(), "w0");
            let sidebar = connected.view.clone();
            assert_eq!(
                view.activity_views()
                    .into_iter()
                    .map(|(host, _)| host)
                    .collect::<Vec<_>>(),
                [Host::Sidebar]
            );
            // Back on Workspaces, the bar's counts keep reading it.
            view.show_activity_panel(ActivityPanel::Workspaces, cx);
            view.poll_orchestrators(window, cx);
            assert!(view.activity.orchestrator.is_some());
            // Opening the tab hands it the same view rather than a second one.
            let checkout = connected_checkout(view);
            view.open_orchestrator_in("w0".into(), Some(checkout), window, cx);
            assert!(view.activity.orchestrator.is_none());
            assert_eq!(view.orchestrators.len(), 1);
            assert!(
                view.orchestrators
                    .values()
                    .any(|orchestrator| orchestrator.view() == &sidebar)
            );
            view.poll_orchestrators(window, cx);
            assert!(view.activity.orchestrator.is_none());
        })
    });
}

fn connected_checkout(view: &crate::HerdrWindow) -> String {
    view.activity
        .orchestrator
        .as_ref()
        .unwrap()
        .checkout()
        .to_owned()
}
