#![allow(clippy::unwrap_used)]
use super::*;
use core::prelude::v1::test;
use herdr_client::ConnectTarget;
use std::sync::Arc;

#[gpui::test]
fn chat_stays_bound_to_its_pane_and_refuses_another_callers_pane(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.endpoints[0].connection.target = ConnectTarget::Session {
                name: "fixture".into(),
                development: false,
            };
            let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
            snapshot.focused_pane_id = Some("p0".into());
            snapshot.focused_tab_id = Some("t0".into());
            snapshot.focused_workspace_id = Some("w0".into());
            view.ensure_layout();
            view.poll_chat(cx);
            assert_eq!(
                view.chat_mode(Some(Mode::Chat), Some("p0"), window, cx)
                    .unwrap()
                    .0,
                Mode::Chat
            );
            assert!(matches!(
                view.chat_mode(None, Some("p1"), window, cx),
                Err(Error::TargetChanged)
            ));
            Arc::make_mut(view.live.snapshot.as_mut().unwrap()).focused_pane_id = Some("p1".into());
            view.poll_chat(cx);
            assert_eq!(view.chat_mode(None, None, window, cx).unwrap().1, "p0");
            // A new daemon preserves the view but requires an explicit rebind.
            let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
            snapshot.boot_id = "new-boot".into();
            snapshot.focused_pane_id = Some("p0".into());
            view.poll_chat(cx);
            assert_eq!(
                view.chat_mode(None, None, window, cx).unwrap().0,
                Mode::Chat
            );
        })
    });
}

#[gpui::test]
fn remote_and_explicit_socket_targets_remain_terminal_only(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            Arc::make_mut(view.live.snapshot.as_mut().unwrap()).focused_pane_id = Some("p0".into());
            assert!(matches!(
                view.chat_mode(Some(Mode::Chat), None, window, cx),
                Err(Error::Unsupported)
            ));
            view.endpoints[0].connection.target = ConnectTarget::Ssh {
                target: "fixture".into(),
                session: "default".into(),
            };
            assert!(matches!(
                view.chat_mode(Some(Mode::Chat), None, window, cx),
                Err(Error::Unsupported)
            ));
        })
    });
}
