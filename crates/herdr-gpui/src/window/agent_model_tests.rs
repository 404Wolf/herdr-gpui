//! The focused agent's model at the start of the status bar: its label, and
//! which agents it opens a model picker for.

#![allow(clippy::unwrap_used)]

use super::{
    HerdrWindow,
    status_bar::{has_model_picker, model_label},
};
use crate::sidebar::layout_tests::{fixture_window, full_draw};
use gpui::TestAppContext;
use herdr_client::protocol::ClientShellAgent;
use std::sync::Arc;

fn agent(kind: &str, tokens: &[(&str, &str)]) -> ClientShellAgent {
    let mut agent = crate::sidebar::layout_tests::snapshot(1).agents.remove(0);
    agent.agent = Some(kind.into());
    agent.tokens = tokens
        .iter()
        .map(|(name, value)| ((*name).into(), (*value).into()))
        .collect();
    agent
}

#[test]
fn the_label_is_the_model_then_the_effort() {
    let agent = agent("claude", &[("model", "Opus 5.5"), ("effort", "high")]);
    assert_eq!(model_label(&agent).as_deref(), Some("Opus 5.5 \u{b7} high"));
}

#[test]
fn the_label_is_the_model_alone_without_an_effort() {
    let agent = agent("codex", &[("model", "gpt-5")]);
    assert_eq!(model_label(&agent).as_deref(), Some("gpt-5"));
}

#[test]
fn there_is_no_label_without_a_model() {
    let agent = agent("claude", &[("effort", "high"), ("pr", "#12")]);
    assert_eq!(model_label(&agent), None);
}

/// `/model` is typed only into Claude: another agent could take the text
/// as a prompt.
#[test]
fn only_claude_gets_the_model_picker() {
    assert!(has_model_picker(&agent("claude", &[])));
    for other in ["codex", "opencode", "pi"] {
        assert!(!has_model_picker(&agent(other, &[])));
    }
    let mut unknown = agent("claude", &[]);
    unknown.agent = None;
    assert!(!has_model_picker(&unknown));
}

fn set_first_agent(
    view: &gpui::Entity<HerdrWindow>,
    focused: bool,
    tokens: &[(&str, &str)],
    cx: &mut gpui::VisualTestContext,
) {
    let tokens: Vec<(String, String)> = tokens
        .iter()
        .map(|(name, value)| ((*name).into(), (*value).into()))
        .collect();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
            snapshot.agents[0].focused = focused;
            snapshot.agents[0].tokens = tokens;
            cx.notify();
        });
    });
}

fn model_segment_drawn(cx: &mut gpui::VisualTestContext) -> bool {
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    cx.debug_bounds("agent-model").is_some()
}

#[gpui::test]
fn the_bar_shows_the_focused_agents_model_only(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    assert!(!model_segment_drawn(cx));

    set_first_agent(&view, true, &[("model", "Opus 5.5")], cx);
    assert!(model_segment_drawn(cx));

    set_first_agent(&view, false, &[("model", "Opus 5.5")], cx);
    assert!(!model_segment_drawn(cx));

    set_first_agent(&view, true, &[], cx);
    assert!(!model_segment_drawn(cx));
}
