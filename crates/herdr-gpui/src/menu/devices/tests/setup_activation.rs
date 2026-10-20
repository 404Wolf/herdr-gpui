use super::*;
use crate::{connection::ConnectionBridge, endpoint::Endpoint, state::ConnectionStatus};
use herdr_client::protocol::ClientShellSnapshot;
use std::sync::Arc;

/// Herdr refuses `workspace.create` from a connection whose surface is
/// inactive, as Local's is while the window shows another device. The setup
/// moves the window to Local and asks only once Local's surface is requested.
#[gpui::test]
fn setup_space_waits_for_the_local_surface(cx: &mut gpui::TestAppContext) {
    let mut peer = crate::window::MockPeer::advertising(&["workspace.create"]);
    let (view, cx) = cx.add_window_view(fixture_window);
    let snapshot: ClientShellSnapshot = serde_json::from_str(include_str!(
        "../../../../../herdr-protocol/tests/fixtures/endpoint-snapshot-v1.json"
    ))
    .unwrap();
    // The bridge traded away shares the peer's handle; dropping it would
    // disconnect the peer before the request is read.
    let _traded = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.endpoints.push(Endpoint::new(
                "remote".into(),
                "Remote".into(),
                herdr_client::ConnectTarget::Socket("/unused-remote.sock".into()),
                false,
            ));
            view.selected_endpoint = 1;
            let local = &mut view.endpoints[0];
            local.connection.handle = Some(peer.client.handle.clone());
            local.live.snapshot = Some(Arc::new(snapshot));
            local.live.status = ConnectionStatus::Connected;
            assert!(!local.surface_requested());

            let request = setup::Request::new("dev@box", "Box", "").unwrap();
            open_form(view, Step::Verifying(request, Offer::Update), cx);
            view.setup_space_verified(
                Ok((
                    setup::Claim::fixture("activation.test"),
                    Ok(setup::TerminalSetup::fixture(
                        "'herdr-gpui' '__device-setup-attach' 'x'",
                    )),
                )),
                cx,
            );
            assert_eq!(view.selected_endpoint, 0);
            // Switching closes dialogs; this one stays open over Local.
            assert_eq!(view.menu.page, Some(Page::AddDevice));
            let form = view.menu.device_setup.as_ref().unwrap();
            assert!(
                matches!(form.step, Step::Activating(..)),
                "{:?}",
                form.error
            );
            assert!(form.claim.is_some());
            // Nothing is asked of the inactive surface.
            view.poll_device_setup(window, cx);
            assert!(matches!(step(view), Step::Activating(..)));
            assert!(
                view.endpoints[0]
                    .connection
                    .inbox
                    .lock()
                    .unwrap()
                    .dialog_response
                    .is_none()
            );

            // The window's poll queues the activation, then the request.
            let local = &mut view.endpoints[0];
            let mut connection = ConnectionBridge::new(local.connection.target.clone());
            connection.handle = Some(peer.client.handle.clone());
            let mut live = local.live.clone();
            local.trade_connection(&mut connection, &mut live);
            view.poll_device_setup(window, cx);
            assert!(matches!(step(view), Step::Opening(..)));
            connection
        })
    });
    assert_eq!(peer.request()["method"], "workspace.create");
}

#[gpui::test]
fn setup_space_fails_when_local_is_disconnected_elsewhere(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.endpoints.push(Endpoint::new(
                "remote".into(),
                "Remote".into(),
                herdr_client::ConnectTarget::Socket("/unused-remote.sock".into()),
                false,
            ));
            view.selected_endpoint = 1;
            view.endpoints[0].live.status = ConnectionStatus::Disconnected;
            let request = setup::Request::new("dev@box", "Box", "").unwrap();
            open_form(view, Step::Verifying(request, Offer::Update), cx);
            view.setup_space_verified(
                Ok((
                    setup::Claim::fixture("activation-down.test"),
                    Ok(setup::TerminalSetup::fixture(
                        "'herdr-gpui' '__device-setup-attach' 'x'",
                    )),
                )),
                cx,
            );
            // No wait for a surface that cannot come: back to the question.
            view.poll_device_setup(window, cx);
            assert!(matches!(step(view), Step::Confirm(_, Offer::Update)));
            assert!(
                view.menu
                    .device_setup
                    .as_ref()
                    .unwrap()
                    .error
                    .as_deref()
                    .unwrap()
                    .starts_with("Open a local workspace:")
            );
        });
    });
}
