//! The chat view over a pane: a header, the entries, what waits on the user,
//! and the composer. Everything is drawn from the session and the bodies
//! prepared when entries changed; nothing here reads a socket. Program text
//! is untrusted and only ever drawn as text.

mod entries;
mod panels;
mod settings;

use super::chats::ChatPane;
use crate::{
    HerdrWindow,
    config::{Theme, corners},
    fonts::StyledFont,
};
use gpui::{prelude::*, *};
use herdr_chat_protocol::{Command, Status};
use herdr_client::protocol::{PaneSurfaceFrame, SurfaceRect};

/// Where a pane's cells are, relative to the terminal canvas: the box the
/// chat view paints in and takes the pointer over, and that its composer
/// places the IME in.
pub(crate) fn pane_bounds(rect: SurfaceRect, cell_width: f32, cell_height: f32) -> Bounds<Pixels> {
    Bounds::new(
        point(
            px(f32::from(rect.x) * cell_width),
            px(f32::from(rect.y) * cell_height),
        ),
        size(
            px(f32::from(rect.width) * cell_width),
            px(f32::from(rect.height) * cell_height),
        ),
    )
}

/// Theme colors the view uses, by meaning.
pub(super) struct Hues {
    pub(super) blue: u32,
    pub(super) green: u32,
    pub(super) red: u32,
    pub(super) yellow: u32,
}

impl Hues {
    pub(super) fn of(theme: &Theme) -> Self {
        Self {
            blue: theme.ink(theme.palette[4]),
            green: theme.ink(theme.palette[2]),
            red: theme.ink(theme.palette[1]),
            yellow: theme.ink(theme.palette[3]),
        }
    }
}

fn status_label(status: &Status) -> &'static str {
    match status {
        Status::Starting => "Starting",
        Status::Idle => "Idle",
        Status::Working => "Working",
        Status::Failed { .. } => "Failed",
        Status::Exited { .. } => "Exited",
    }
}

/// The status, with how long the running turn has taken once it passes a
/// second.
pub(super) fn status_text(transcript: &herdr_chat_protocol::Transcript) -> String {
    let label = status_label(&transcript.status);
    match transcript.turn {
        Some(turn) if transcript.status == Status::Working => {
            let elapsed = super::now_ms().saturating_sub(turn.started_ms);
            if elapsed < 1000 {
                return label.to_owned();
            }
            format!(
                "{label} {}",
                herdr_chat_protocol::display::duration(elapsed)
            )
        }
        _ => label.to_owned(),
    }
}

impl HerdrWindow {
    /// The chat views over the panes of the frame on screen. A popup covers
    /// the panes, so none are drawn under it.
    pub(crate) fn render_chats(
        &self,
        surface: Option<&PaneSurfaceFrame>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(surface) = surface.filter(|surface| surface.popup.is_none()) else {
            return Vec::new();
        };
        let cell_height = self.config.terminal.line_height();
        surface
            .panes
            .iter()
            .filter_map(|pane| {
                let chat = self.chats.pane(&pane.pane_id)?;
                if !chat.available() {
                    return None;
                }
                let bounds = pane_bounds(pane.inner_rect, self.cell_width, cell_height);
                Some(if self.chats.raw(&pane.pane_id) {
                    self.render_chat_switch(&pane.pane_id, bounds, cx)
                } else {
                    self.render_chat(chat, bounds, cx)
                })
            })
            .collect()
    }

    /// The small button over a pane turned back to its terminal, which
    /// brings the chat view back.
    fn render_chat_switch(
        &self,
        pane_id: &str,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &self.theme;
        let pane = pane_id.to_owned();
        div()
            .absolute()
            .left(bounds.origin.x)
            .top(bounds.origin.y)
            .w(bounds.size.width)
            .flex()
            .justify_end()
            .p(px(6.))
            .child(
                div()
                    .id(SharedString::from(format!("chat-switch-{pane_id}")))
                    .debug_selector(|| "chat-switch".into())
                    .occlude()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(corners::CONTROL))
                    .border_1()
                    .border_color(rgb(theme.active))
                    .bg(rgb(theme.surface))
                    .text_color(rgb(theme.foreground))
                    .text_size(px(self.config.ui.size))
                    .text_font(&self.config.ui)
                    .cursor_pointer()
                    .hover(|button| button.bg(rgb(theme.active)))
                    .child(
                        svg()
                            .path("icons/note.svg")
                            .size(px(12.))
                            .text_color(rgb(theme.foreground)),
                    )
                    .child("Chat")
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        this.set_chat_raw(&pane, false, window, cx);
                    })),
            )
            .into_any_element()
    }

    fn render_chat(
        &self,
        chat: &ChatPane,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &self.theme;
        let pane_id = chat.target.pane_id.clone();
        let entries = {
            let pane_id = pane_id.clone();
            list(
                chat.list.clone(),
                cx.processor(move |this, index: usize, _, cx| {
                    this.render_chat_entry(&pane_id, index, cx)
                }),
            )
            .flex_1()
            .min_h_0()
            .py(px(8.))
        };
        let focus_pane = pane_id.clone();
        let key_pane = pane_id.clone();
        let capture_pane = pane_id.clone();
        div()
            .id(SharedString::from(format!("chat-{pane_id}")))
            .debug_selector(|| "chat-view".into())
            .absolute()
            .left(bounds.origin.x)
            .top(bounds.origin.y)
            .w(bounds.size.width)
            .h(bounds.size.height)
            .occlude()
            .overflow_hidden()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .text_size(px(self.config.ui.size))
            .text_font(&self.config.ui)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.focus_chat(&focus_pane, window, cx);
                }),
            )
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                this.chat_key_down(&key_pane, event, window, cx);
            }))
            .capture_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                this.chat_capture_key(&capture_pane, event, window, cx);
            }))
            .child(self.render_chat_header(chat, cx))
            .child(entries)
            .children(self.render_chat_plan(chat))
            .children(self.render_chat_permissions(chat, cx))
            .children(self.render_chat_queue(chat, cx))
            .children(self.render_chat_completions(chat, cx))
            .child(self.render_chat_composer(chat))
            .children(
                chat.menu
                    .clone()
                    .map(|menu| self.render_chat_menu(chat, &menu, cx)),
            )
            .into_any_element()
    }

    /// Gives the chat's composer the keyboard and makes its pane Herdr's
    /// focused one.
    fn focus_chat(&mut self, pane_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.note_input();
        if let Some(input) = self
            .chats
            .pane(pane_id)
            .and_then(|pane| pane.composer.clone())
        {
            let focus = input.read(cx).focus.clone();
            window.focus(&focus, cx);
        }
        if self.input_ready() {
            self.focus_clicked_pane(pane_id, cx);
        }
        if let Some(pane) = self.chats.pane_mut(pane_id)
            && pane.menu.is_some()
        {
            pane.menu = None;
            cx.notify();
        }
    }

    fn render_chat_header(&self, chat: &ChatPane, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let hues = Hues::of(theme);
        let transcript = chat.session.transcript();
        let status = &transcript.status;
        let dot = match status {
            Status::Starting => theme.muted,
            Status::Idle => hues.green,
            Status::Working => hues.yellow,
            Status::Failed { .. } | Status::Exited { .. } => hues.red,
        };
        let pane_id = chat.target.pane_id.clone();
        let button = |id: &'static str| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(6.))
                .h(px(22.))
                .rounded(px(corners::CONTROL))
                .cursor_pointer()
                .hover(|button| button.bg(rgb(theme.active)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let settings = self.render_chat_settings_buttons(chat, cx);
        let usage = self.render_chat_usage(chat);
        let cancel = (*status == Status::Working).then(|| {
            let pane_id = pane_id.clone();
            button("chat-cancel")
                .text_color(rgb(hues.red))
                .child("Stop")
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    this.chat_send(&pane_id, Command::Cancel, cx);
                }))
        });
        let watching = (transcript.clients > 1).then(|| {
            div()
                .flex_none()
                .text_color(rgb(theme.subtext()))
                .child(format!("{} watching", transcript.clients))
        });
        let reconnecting = (!chat.attached()).then(|| {
            div()
                .flex_none()
                .text_color(rgb(hues.yellow))
                .child("Reconnecting\u{2026}")
        });
        let terminal = {
            let pane_id = pane_id.clone();
            button("chat-terminal")
                .child(
                    svg()
                        .path("icons/terminal.svg")
                        .size(px(13.))
                        .text_color(rgb(theme.foreground)),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    this.set_chat_raw(&pane_id, true, window, cx);
                }))
        };
        let message = match status {
            Status::Failed { message } | Status::Exited { message } if !message.is_empty() => {
                Some(crate::notifications::safe_text(message, 400))
            }
            _ => None,
        };
        div()
            .flex_none()
            .flex()
            .flex_col()
            .border_b_1()
            .border_color(rgb(theme.active))
            .bg(rgb(theme.surface))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .h(px(34.))
                    .child(div().flex_none().size(px(8.)).rounded_full().bg(rgb(dot)))
                    .child(
                        div()
                            .debug_selector(|| "chat-agent".into())
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(crate::notifications::safe_text(chat.session.agent(), 80)),
                    )
                    .child(
                        div()
                            .debug_selector(|| "chat-status".into())
                            .flex_none()
                            .text_color(rgb(theme.subtext()))
                            .child(status_text(transcript)),
                    )
                    .child(div().flex_1())
                    .children(reconnecting)
                    .children(watching)
                    .children(usage)
                    .children(cancel)
                    .child(terminal),
            )
            .children((!settings.is_empty()).then(|| {
                div()
                    .debug_selector(|| "chat-settings".into())
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .pb(px(6.))
                    .children(settings)
            }))
            .children(message.map(|message| {
                div()
                    .px(px(10.))
                    .pb(px(6.))
                    .text_color(rgb(hues.red))
                    .child(message)
            }))
    }
}
