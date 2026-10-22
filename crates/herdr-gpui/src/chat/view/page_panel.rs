//! The panel beside the entries that shows the page the user previewed,
//! in place of the rail. Its header names the page and closes it; its body
//! is the page itself, which on macOS shows from beneath the window's
//! drawing, so the body paints nothing of its own.

use super::super::{chats::ChatPane, page::MIN_PANE};
use crate::{HerdrWindow, config::corners, notifications::safe_text};
use gpui::{prelude::*, *};

/// The panel's share of the pane's width, and the least it takes.
const SHARE: f32 = 0.5;
const MIN_WIDTH: f32 = 320.;

impl HerdrWindow {
    /// The page panel, when the chat shows a page and the pane has room.
    pub(super) fn render_chat_page_panel(
        &self,
        chat: &ChatPane,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let page = chat.page.as_ref()?;
        if bounds.size.width < px(MIN_PANE) {
            return None;
        }
        let theme = &self.theme;
        let width = (bounds.size.width * SHARE).max(px(MIN_WIDTH));
        let pane_id = chat.target.pane_id.clone();
        let close = div()
            .id("chat-page-close")
            .debug_selector(|| "chat-page-close".into())
            .flex_none()
            .size(px(22.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(corners::CONTROL))
            .cursor_pointer()
            .hover(|button| button.bg(rgb(theme.active)))
            .child(
                svg()
                    .path("icons/close.svg")
                    .size(px(10.))
                    .text_color(rgb(theme.subtext())),
            )
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                this.close_chat_panel(&pane_id, cx);
            }));
        #[cfg(any(target_os = "macos", windows))]
        let body = match self.chat_page_element(page.id) {
            Some(page) => div().size_full().child(page),
            None => opening(theme),
        };
        #[cfg(not(any(target_os = "macos", windows)))]
        let body = opening(theme);
        Some(
            div()
                .debug_selector(|| "chat-page".into())
                .flex_none()
                .w(width)
                .h_full()
                .flex()
                .flex_col()
                .border_l_1()
                .border_color(rgb(theme.active))
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(10.))
                        .h(px(32.))
                        .border_b_1()
                        .border_color(rgb(theme.active))
                        .bg(rgb(theme.background))
                        .child(
                            svg()
                                .flex_none()
                                .path("icons/globe.svg")
                                .size(px(12.))
                                .text_color(rgb(theme.subtext())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(safe_text(&page.title, 120)),
                        )
                        .child(close),
                )
                .child(div().flex_1().min_h_0().relative().child(body)),
        )
    }
}

fn opening(theme: &crate::config::Theme) -> Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(theme.background))
        .text_color(rgb(theme.muted))
        .child("Opening the page\u{2026}")
}
