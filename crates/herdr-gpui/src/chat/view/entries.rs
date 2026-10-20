//! One transcript entry as a row of the chat's list.

use super::{super::prepared::Body, Hues};
use crate::{HerdrWindow, config::corners, fonts::StyledFont, notifications::safe_text};
use gpui::{prelude::*, *};
use herdr_chat_protocol::{Entry, EntryKind, NoticeLevel, Tool, ToolKind, ToolStatus};

/// Locations listed under a tool call.
const LOCATIONS: usize = 3;
/// Characters of a folded thought shown.
const FOLDED: usize = 120;

fn tool_kind(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "Read",
        ToolKind::Edit => "Edit",
        ToolKind::Delete => "Delete",
        ToolKind::Move => "Move",
        ToolKind::Search => "Search",
        ToolKind::Execute => "Run",
        ToolKind::Think => "Think",
        ToolKind::Fetch => "Fetch",
        ToolKind::SwitchMode => "Mode",
        ToolKind::Other => "Tool",
    }
}

impl HerdrWindow {
    pub(super) fn render_chat_entry(
        &self,
        pane_id: &str,
        index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(chat) = self.chats.pane(pane_id) else {
            return div().into_any_element();
        };
        let Some(entry) = chat.session.transcript().entries.get(index) else {
            return div().into_any_element();
        };
        let body = chat.prepared.body(entry.id);
        let content = match &entry.kind {
            EntryKind::User { text } => self.render_chat_user(text),
            EntryKind::Agent { text } => {
                let message = match body.as_deref() {
                    Some(Body::Markdown(lines)) => crate::release_notes::render(
                        "chat-agent",
                        lines,
                        &self.theme,
                        &self.config.terminal,
                    ),
                    _ => div().child(text.clone()),
                };
                div()
                    .group("chat-agent-message")
                    .child(message)
                    .child(self.render_chat_copy(entry.id.0, text, cx))
            }
            EntryKind::Turn(summary) => self.render_chat_turn(&summary.describe()),
            EntryKind::Thought { text } => self.render_chat_thought(
                pane_id,
                entry,
                text,
                chat.expanded.contains(&entry.id),
                cx,
            ),
            EntryKind::Tool(tool) => self.render_chat_tool(tool, body.as_deref()),
            EntryKind::Notice { level, text } => self.render_chat_notice(*level, text),
        };
        div()
            .id(("chat-entry", entry.id.0))
            .px(px(12.))
            .pt(px(8.))
            .min_w_0()
            .child(content)
            .into_any_element()
    }

    /// A copy button under an agent message, shown while the pointer is on it.
    fn render_chat_copy(&self, id: u64, text: &str, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let text = text.to_owned();
        div().h(px(18.)).flex().child(
            div()
                .id(("chat-copy", id))
                .debug_selector(move || format!("chat-copy-{id}"))
                .invisible()
                .group_hover("chat-agent-message", |button| button.visible())
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(4.))
                .rounded(px(corners::SMALL))
                .cursor_pointer()
                .text_size(px((self.config.ui.size - 2.).max(10.)))
                .text_color(rgb(theme.muted))
                .hover(|button| button.bg(rgb(theme.active)))
                .child(
                    svg()
                        .path("icons/copy.svg")
                        .size(px(11.))
                        .text_color(rgb(theme.muted)),
                )
                .child("Copy")
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    this.show_flash(crate::window::Flash::success("Copied the message"), cx);
                })),
        )
    }

    /// How a turn ended, as a quiet rule across the list.
    fn render_chat_turn(&self, text: &str) -> Div {
        let theme = &self.theme;
        let rule = || div().flex_1().h(px(1.)).bg(rgb(theme.active));
        div()
            .debug_selector(|| "chat-turn".into())
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px((self.config.ui.size - 1.).max(10.)))
            .text_color(rgb(theme.muted))
            .child(rule())
            .child(div().flex_none().child(text.to_owned()))
            .child(rule())
    }

    fn render_chat_user(&self, text: &str) -> Div {
        let theme = &self.theme;
        div().flex().justify_end().child(
            div()
                .max_w(relative(0.85))
                .px(px(10.))
                .py(px(6.))
                .rounded(px(corners::CONTROL))
                .bg(rgb(theme.active))
                .child(text.to_owned()),
        )
    }

    fn render_chat_thought(
        &self,
        pane_id: &str,
        entry: &Entry,
        text: &str,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        let shown = if expanded {
            text.to_owned()
        } else {
            let line = text
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("");
            safe_text(line, FOLDED)
        };
        let pane_id = pane_id.to_owned();
        let id = entry.id;
        div().child(
            div()
                .id(("chat-thought", id.0))
                .flex()
                .gap(px(6.))
                .cursor_pointer()
                .text_color(rgb(theme.dimmed(theme.foreground)))
                .italic()
                .child(
                    svg()
                        .path(if expanded {
                            "icons/chevron-down.svg"
                        } else {
                            "icons/chevron-right.svg"
                        })
                        .flex_none()
                        .mt(px(3.))
                        .size(px(10.))
                        .text_color(rgb(theme.muted)),
                )
                .child(
                    div()
                        .min_w_0()
                        .when(!expanded, |row| row.truncate())
                        .child(shown),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    if let Some(pane) = this.chats.pane_mut(&pane_id) {
                        if !pane.expanded.remove(&id) {
                            pane.expanded.insert(id);
                        }
                        pane.remeasure(id);
                    }
                    cx.notify();
                })),
        )
    }

    fn render_chat_tool(&self, tool: &Tool, body: Option<&Body>) -> Div {
        let theme = &self.theme;
        let hues = Hues::of(theme);
        let (status, color) = match tool.status {
            ToolStatus::Pending => ("pending", theme.muted),
            ToolStatus::InProgress => ("running", hues.yellow),
            ToolStatus::Completed => ("done", hues.green),
            ToolStatus::Failed => ("failed", hues.red),
        };
        let small = px((self.config.ui.size - 1.).max(10.));
        let summary = match body {
            Some(Body::Tool(summary)) => Some(summary),
            _ => None,
        };
        let more_locations = tool.locations.len().saturating_sub(LOCATIONS);
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(8.))
            .py(px(4.))
            .rounded(px(corners::SMALL))
            .border_l_2()
            .border_color(rgb(color))
            .bg(rgb(theme.surface))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(hues.blue))
                            .child(tool_kind(tool.kind)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(safe_text(&tool.title, 300)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(small)
                            .text_color(rgb(color))
                            .child(status),
                    ),
            )
            .children(tool.locations.iter().take(LOCATIONS).map(|location| {
                div()
                    .text_size(small)
                    .text_color(rgb(theme.subtext()))
                    .truncate()
                    .child(safe_text(location, 300))
            }))
            .children((more_locations > 0).then(|| {
                div()
                    .text_size(small)
                    .text_color(rgb(theme.muted))
                    .child(format!("and {more_locations} more"))
            }))
            .children(summary.into_iter().flat_map(|summary| {
                let diffs = summary.diffs.iter().map(|diff| {
                    div()
                        .flex()
                        .gap(px(6.))
                        .text_size(small)
                        .child(div().min_w_0().truncate().child(safe_text(&diff.path, 300)))
                        .child(
                            div()
                                .flex_none()
                                .text_color(rgb(hues.green))
                                .child(format!("+{}", diff.added)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_color(rgb(hues.red))
                                .child(format!("\u{2212}{}", diff.removed)),
                        )
                });
                let lines = summary.lines.iter().map(|line| {
                    div()
                        .text_size(small)
                        .text_font(&self.config.terminal)
                        .text_color(rgb(theme.subtext()))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(line.clone())
                });
                let more = (summary.more > 0).then(|| {
                    div()
                        .text_size(small)
                        .text_color(rgb(theme.muted))
                        .child(format!("{} more lines", summary.more))
                });
                let terminals = (summary.terminals > 0).then(|| {
                    div()
                        .text_size(small)
                        .text_color(rgb(theme.muted))
                        .child("Terminal output shows in the program's own view")
                });
                diffs
                    .chain(lines)
                    .chain(more)
                    .chain(terminals)
                    .collect::<Vec<_>>()
            }))
    }

    fn render_chat_notice(&self, level: NoticeLevel, text: &str) -> Div {
        let theme = &self.theme;
        let hues = Hues::of(theme);
        let color = match level {
            NoticeLevel::Info => theme.subtext(),
            NoticeLevel::Warning => hues.yellow,
            NoticeLevel::Error => hues.red,
        };
        div()
            .pl(px(8.))
            .border_l_2()
            .border_color(rgb(color))
            .text_color(rgb(color))
            .child(text.to_owned())
    }
}
