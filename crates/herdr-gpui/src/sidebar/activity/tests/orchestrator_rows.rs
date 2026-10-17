//! Rows of an orchestrator tab open on another device than the one shown.
use crate::{
    browser::{Location, OrchestratorRepo, Store, scope},
    sidebar::layout_tests::{fixture_window, snapshot},
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
