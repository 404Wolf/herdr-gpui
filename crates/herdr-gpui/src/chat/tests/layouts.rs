//! Chat layouts: each draws the same transcript and composer its own way,
//! and switching one measures the rows again.

use super::{
    overlay::{attach_with, open, poll},
    timeline::conversation,
    *,
};
use crate::{HerdrWindow, chat::chats::Chats, config::ChatLayoutMode};
use gpui::{Entity, TestAppContext, VisualTestContext};
use herdr_client::ConnectTarget;

fn switch(view: &Entity<HerdrWindow>, mode: ChatLayoutMode, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.set_chat_layout_with(mode, |_| Ok(()), cx)
    });
}

#[gpui::test]
fn bubbles_put_the_user_on_the_right_without_avatars(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let _stream = attach_with(&view, &program, conversation(false), cx);
    for mode in [ChatLayoutMode::Bubbles, ChatLayoutMode::Pill] {
        switch(&view, mode, cx);
        poll(&view, &program, cx);
        assert!(cx.debug_bounds("chat-avatar-1").is_none(), "{mode:?}");
        assert!(cx.debug_bounds("chat-speaker").is_none(), "{mode:?}");
        assert!(cx.debug_bounds("chat-dot-3").is_none(), "{mode:?}");
        let bubble = cx.debug_bounds("chat-bubble-1").unwrap();
        let chat = cx.debug_bounds("chat-view").unwrap();
        assert!(
            bubble.center().x > chat.center().x,
            "{mode:?}: on the right"
        );
        // Only the user's words take a bubble.
        assert!(cx.debug_bounds("chat-bubble-2").is_none(), "{mode:?}");
        for part in ["chat-composer", "chat-settings", "chat-send", "chat-attach"] {
            assert!(cx.debug_bounds(part).is_some(), "{mode:?}: {part}");
        }
    }
    switch(&view, ChatLayoutMode::Timeline, cx);
    poll(&view, &program, cx);
    assert!(cx.debug_bounds("chat-avatar-1").is_some());
    assert!(cx.debug_bounds("chat-bubble-1").is_none());
}

#[gpui::test]
fn the_composer_arranges_its_parts_by_layout(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let _stream = attach_with(&view, &program, conversation(false), cx);
    let parts = |cx: &mut VisualTestContext| {
        (
            cx.debug_bounds("chat-composer").unwrap(),
            cx.debug_bounds("chat-attach").unwrap(),
            cx.debug_bounds("chat-send").unwrap(),
        )
    };
    // The timeline's card keeps its buttons along the foot, under the draft.
    let (draft, attach, send) = parts(cx);
    assert!(attach.top() >= draft.bottom());
    assert!(send.top() >= draft.bottom());

    // Bubbles: send beside the draft, the settings under the box.
    switch(&view, ChatLayoutMode::Bubbles, cx);
    poll(&view, &program, cx);
    let (draft, attach, send) = parts(cx);
    assert!(send.left() >= draft.right(), "send beside the draft");
    assert!(attach.top() > draft.bottom(), "settings under the box");

    // Pill: attach leads the draft and send closes the row.
    switch(&view, ChatLayoutMode::Pill, cx);
    poll(&view, &program, cx);
    let (draft, attach, send) = parts(cx);
    assert!(attach.right() <= draft.left(), "attach leads");
    assert!(send.left() >= draft.right(), "send closes");
    assert!(attach.top() < draft.bottom() && attach.bottom() > draft.top());
}

#[gpui::test]
fn the_header_names_the_layout(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let _stream = attach_with(&view, &program, conversation(false), cx);
    assert!(cx.debug_bounds("chat-layout").is_some());
    switch(&view, ChatLayoutMode::Pill, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.config.chat.layout),
        ChatLayoutMode::Pill
    );
}

#[test]
fn a_new_layout_measures_the_rows_again_once() {
    let mut chats = Chats::default();
    assert!(
        !chats.relayout(ChatLayoutMode::Timeline),
        "already measured"
    );
    assert!(chats.relayout(ChatLayoutMode::Bubbles));
    assert!(!chats.relayout(ChatLayoutMode::Bubbles));
}

#[test]
fn the_layouts_step_round() {
    let mut mode = ChatLayoutMode::default();
    let mut seen = Vec::new();
    for _ in ChatLayoutMode::ALL {
        seen.push(mode);
        mode = mode.next();
    }
    assert_eq!(mode, ChatLayoutMode::default());
    assert_eq!(seen, ChatLayoutMode::ALL);
}
