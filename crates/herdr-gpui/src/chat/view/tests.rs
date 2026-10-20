#![allow(clippy::unwrap_used)]
use super::*;
use core::prelude::v1::test;

#[gpui::test]
fn a_verified_session_without_its_first_transcript_has_an_empty_state(cx: &mut TestAppContext) {
    let (view, cx) =
        cx.add_window_view(|_, cx| View::fixture(Config::default(), Theme::default(), cx));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.conversation = Some(Arc::new(Conversation::default()));
            view.rows = Arc::from([]);
            view.rebuild();
            cx.notify();
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("chat-empty").is_some());
    assert!(cx.debug_bounds("chat-composer").is_some());
}

#[gpui::test]
fn switching_modes_keeps_draft_and_moves_focus_off_hidden_composer(cx: &mut TestAppContext) {
    let (view, cx) =
        cx.add_window_view(|_, cx| View::fixture(Config::default(), Theme::default(), cx));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.composer.update(cx, |input, cx| {
                input.replace_text_in_range(None, "draft\n😀", window, cx)
            });
            view.expanded.insert("response:tools".into());
            view.rebuild();
            view.set_mode(Mode::Chat, window, cx);
            assert!(view.composer.read(cx).focus.is_focused(window));
            view.set_mode(Mode::Terminal, window, cx);
            assert!(!view.composer.read(cx).focus.is_focused(window));
            view.set_mode(Mode::Chat, window, cx);
            assert_eq!(view.composer.read(cx).text(), "draft\n😀");
            assert!(view.expanded.contains("response:tools"));
        })
    });
}

#[gpui::test]
fn history_changes_keep_a_scrolled_anchor_and_only_paint_visible_rows(cx: &mut TestAppContext) {
    let (view, cx) =
        cx.add_window_view(|_, cx| View::fixture(Config::default(), Theme::default(), cx));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.rows = (0..2000)
                .map(|i| Row {
                    key: format!("row-{i}"),
                    group: None,
                    body: Body::Notice(format!("row {i}")),
                })
                .collect::<Vec<_>>()
                .into();
            view.rebuild();
            view.list.scroll_to(ListOffset {
                item_ix: 1000,
                offset_in_item: px(0.),
            });
            cx.notify();
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("chat-row-1000").is_some());
    assert!(cx.debug_bounds("chat-row-0").is_none());
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let before = view.list.logical_scroll_top();
            let mut rows = view.rows.to_vec();
            rows.push(Row {
                key: "new".into(),
                group: None,
                body: Body::Notice("new response".into()),
            });
            view.rows = rows.into();
            view.rebuild();
            let after = view.list.logical_scroll_top();
            assert_eq!(
                (after.item_ix, after.offset_in_item),
                (before.item_ix, before.offset_in_item)
            );
            cx.notify();
        })
    });
}

#[gpui::test]
fn readiness_requires_identity_and_no_automatic_replay_state(cx: &mut TestAppContext) {
    let (view, cx) =
        cx.add_window_view(|_, cx| View::fixture(Config::default(), Theme::default(), cx));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert!(!view.can_send(cx));
            view.identity = Some(Identity {
                terminal: "t1".into(),
                directory: "fixture-claude-store".into(),
                runtime: None,
                session: herdr_client::chat::AgentSession {
                    agent: "claude".into(),
                    kind: herdr_client::chat::SessionKind::Id,
                    source: "herdr:claude".into(),
                    value: "00000000-0000-4000-8000-000000000001".into(),
                },
            });
            assert!(view.can_send(cx));
            view.status = AgentStatus::Working;
            assert!(!view.can_send(cx));
            view.status = AgentStatus::Idle;
            view.connected = false;
            assert!(!view.can_send(cx));
            view.connected = true;
            view.error = Some("Delivery was not confirmed".into());
            assert!(!view.can_send(cx));
        })
    });
}
