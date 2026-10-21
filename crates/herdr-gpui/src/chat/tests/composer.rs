//! The composer keeps a prompt's lines: a pasted or recalled prompt with
//! several lines goes into the draft line by line, and a long draft never
//! pushes the editable line out of the pane.

use super::{
    overlay::{attach_with, open, poll},
    *,
};
use gpui::{ClipboardItem, TestAppContext};
use herdr_chat_protocol::Command;
use herdr_client::ConnectTarget;

fn idle() -> ServerMessage {
    let mut transcript = Transcript::new("Claude");
    transcript.status = Status::Idle;
    transcript.entries = vec![agent(1, "Hi.")];
    ServerMessage::Snapshot {
        cursor: cursor(1, 0),
        transcript: Box::new(transcript),
    }
}

fn prompt_text(stream: &mut Peer) -> String {
    match read(stream) {
        Some(ClientMessage::Command {
            command: Command::Prompt { text, .. },
            ..
        }) => text,
        other => panic!("a prompt, got {other:?}"),
    }
}

fn paste_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-shift-v"
    }
}

#[gpui::test]
fn a_pasted_prompt_keeps_its_lines(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let mut stream = attach_with(&view, &program, idle(), cx);
    cx.simulate_input("see: ");
    cx.write_to_clipboard(ClipboardItem::new_string(
        "fn main() {\r\n    run();\n}".into(),
    ));
    cx.simulate_keystrokes(paste_key());
    poll(&view, &program, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(prompt_text(&mut stream), "see: fn main() {\n    run();\n}");

    // Up brings the whole prompt back, lines and all.
    cx.simulate_keystrokes("up");
    cx.simulate_keystrokes("enter");
    assert_eq!(prompt_text(&mut stream), "see: fn main() {\n    run();\n}");
}

#[gpui::test]
fn a_long_draft_keeps_the_editable_line_in_the_pane(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let _stream = attach_with(&view, &program, idle(), cx);
    let lines: Vec<String> = (0..40).map(|n| format!("line {n}")).collect();
    cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\n")));
    cx.simulate_keystrokes(paste_key());
    poll(&view, &program, cx);
    assert!(cx.debug_bounds("chat-draft-earlier").is_some());
    let chat = cx.debug_bounds("chat-view").unwrap();
    let composer = cx.debug_bounds("chat-composer").unwrap();
    assert!(
        composer.bottom() <= chat.bottom(),
        "{composer:?} in {chat:?}"
    );
}
