//! What a message shows for things loaded only when asked: pages from HTML
//! fences and HTML files, pictures on the web, and the captions of pictures
//! herdr-acp adds after the message.

use super::{
    overlay::{attach_with, open, poll},
    visual::message,
    *,
};
use crate::HerdrWindow;
use crate::{
    browser::TabId,
    chat::{page::ChatPage, visual::VisualId},
};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use herdr_chat_protocol::{Tool, ToolKind, ToolStatus};
use herdr_client::ConnectTarget;

#[test]
fn a_closed_html_fence_becomes_a_page_and_an_open_one_stays_code() {
    let (_, shape) = message("Here it is:\n```html\n<h1>Hi</h1>\n```\nEnjoy.");
    assert_eq!(shape, ["text:Here it is:", "page", "text:Enjoy."]);
    let (_, shape) = message("Here it is:\n```html\n<h1>Hi</h1>\n");
    assert_eq!(shape, ["text:Here it is:"]);
    let long = format!(
        "```html\n{}\n```",
        "x".repeat(crate::chat::prepared::MAX_PAGE + 1)
    );
    let (_, shape) = message(&long);
    assert!(!shape.contains(&"page".to_owned()), "too long to preview");
}

#[test]
fn picture_lines_load_on_request_or_name_what_follows() {
    let (prepared, shape) = message(
        "A chart:\n![chart](out/chart.png)\n![logo](https://example.com/logo.png)\n\
         See ![inline](a.png) here.\n![data](data:image/png;base64,AAAA)\n\
         ![odd](ftp://example.com/a.png)",
    );
    assert_eq!(
        shape,
        [
            "text:A chart:",
            "picture:chart",
            "remote:https://example.com/logo.png",
            "text:See ![inline](a.png) here.",
        ]
    );
    let remote = VisualId::of("https://example.com/logo.png");
    assert!(!prepared.visuals.wants(remote), "nothing loads unasked");
}

/// The pane the fixture's agent runs in, which the chat stands in for.
fn chat_pane(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| {
        view.live.snapshot.as_ref().unwrap().agents[0]
            .pane_id
            .clone()
    })
}

fn said(text: &str) -> ServerMessage {
    let mut transcript = Transcript::new("Claude");
    transcript.status = Status::Idle;
    transcript.entries = vec![agent(1, text)];
    ServerMessage::Snapshot {
        cursor: cursor(1, 0),
        transcript: Box::new(transcript),
    }
}

#[gpui::test]
fn a_web_picture_loads_only_once_asked(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    // Nothing listens there, so asking fails at once without the network.
    let url = "http://127.0.0.1:9/chart.png";
    let _stream = attach_with(&view, &program, said(&format!("![chart]({url})")), cx);
    let id = VisualId::of(url);
    let pane = chat_pane(&view, cx);
    let wants = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.chats.pane(&pane).unwrap().prepared.visuals.wants(id)
        })
    };
    assert!(!wants(cx));
    let load = cx.debug_bounds("chat-load-picture").unwrap();
    cx.simulate_click(load.center(), Modifiers::default());
    assert!(wants(cx));
    poll(&view, &program, cx);
    assert!(cx.debug_bounds("chat-load-picture").is_none());
}

#[gpui::test]
fn the_page_panel_takes_the_rail_s_place_and_closes(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let _stream = attach_with(&view, &program, said("```html\n<p>hi</p>\n```"), cx);
    assert!(cx.debug_bounds("chat-html-0").is_some());
    assert!(cx.debug_bounds("chat-preview").is_some());
    assert!(cx.debug_bounds("chat-page").is_none());
    let pane = chat_pane(&view, cx);
    view.update(cx, |view, _| {
        view.chats.pane_mut(&pane).unwrap().page = Some(ChatPage {
            id: TabId::panel(1),
            title: "HTML page".into(),
        });
    });
    poll(&view, &program, cx);
    let panel = cx.debug_bounds("chat-page").unwrap();
    let chat = cx.debug_bounds("chat-view").unwrap();
    assert_eq!(panel.right(), chat.right(), "on the right");
    assert!(cx.debug_bounds("chat-rail").is_none());
    let close = cx.debug_bounds("chat-page-close").unwrap();
    cx.simulate_click(close.center(), Modifiers::default());
    assert!(view.read_with(cx, |view, _| view.chats.pane(&pane).unwrap().page.is_none()));
}

#[gpui::test]
fn an_html_file_a_tool_wrote_offers_a_preview(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let mut transcript = Transcript::new("Claude");
    transcript.status = Status::Idle;
    let page = if cfg!(windows) {
        r"C:\work\index.html"
    } else {
        "/work/index.html"
    };
    transcript.entries = vec![Entry {
        id: EntryId(1),
        kind: EntryKind::Tool(Tool {
            call_id: "write".into(),
            content: Vec::new(),
            kind: ToolKind::Edit,
            locations: vec![page.into()],
            status: ToolStatus::Completed,
            title: "Write index.html".into(),
        }),
    }];
    let snapshot = ServerMessage::Snapshot {
        cursor: cursor(1, 0),
        transcript: Box::new(transcript),
    };
    let _stream = attach_with(&view, &program, snapshot, cx);
    assert!(cx.debug_bounds("chat-preview").is_some());
}
