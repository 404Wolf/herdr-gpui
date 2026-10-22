//! What an agent message shows in place of things the app loads only when
//! asked: a page from an HTML fence, a picture on the web, and the caption
//! of a picture on the agent's machine that follows the message.

use super::{
    super::{
        page::PageSource,
        visual::{Source, VisualId, Visuals},
    },
    visuals::Frame,
};
use crate::{HerdrWindow, browser::WebUrl, config::corners, notifications::safe_text};
use gpui::{prelude::*, *};
use herdr_chat_protocol::EntryId;
use std::{path::PathBuf, rc::Rc};

/// What Preview says where it opens the page.
const PREVIEW: &str = if crate::browser::EMBEDDED {
    "Preview"
} else {
    "Open in browser"
};

impl HerdrWindow {
    /// A card for an HTML fence: what it is, and a button that previews it.
    pub(super) fn render_chat_html_card(
        &self,
        pane_id: &str,
        index: usize,
        page: &Rc<str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let lines = page.lines().count();
        let source = PageSource::Html(page.clone());
        self.embed_card("icons/globe.svg")
            .debug_selector(move || format!("chat-html-{index}"))
            .child(div().flex_1().min_w_0().child(match lines {
                1 => "HTML page \u{b7} 1 line".to_owned(),
                count => format!("HTML page \u{b7} {count} lines"),
            }))
            .child(self.preview_button(pane_id, ("chat-preview", index), source, cx))
            .into_any_element()
    }

    /// A Preview button for an HTML file a tool call wrote.
    pub(super) fn render_chat_file_preview(
        &self,
        pane_id: &str,
        entry: EntryId,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.preview_button(
            pane_id,
            ("chat-file-preview", entry.0),
            PageSource::File(path),
            cx,
        )
    }

    fn preview_button(
        &self,
        pane_id: &str,
        id: impl Into<ElementId>,
        source: PageSource,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let pane_id = pane_id.to_owned();
        let fill = theme.primary();
        div()
            .id(id)
            .debug_selector(|| "chat-preview".into())
            .flex_none()
            .px(px(10.))
            .h(px(24.))
            .flex()
            .items_center()
            .rounded(px(corners::CONTROL))
            .bg(rgb(fill))
            .text_color(rgb(theme.text_on(fill)))
            .text_size(px((self.chat_text_size() - 1.).max(10.)))
            .cursor_pointer()
            .child(PREVIEW)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                this.preview_chat_page(&pane_id, source.clone(), window, cx);
            }))
    }

    /// A picture on the web: drawn once loaded, until then a card that
    /// names its host and loads it on request.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_chat_remote_picture(
        &self,
        pane_id: &str,
        entry: EntryId,
        index: usize,
        id: VisualId,
        url: &WebUrl,
        alt: &str,
        visuals: &Visuals,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if visuals.wants(id) {
            return self
                .render_chat_visual(id, visuals, Frame::Image)
                .into_any_element();
        }
        let theme = &self.theme;
        let pane_id = pane_id.to_owned();
        let url = url.clone();
        let host = safe_text(url.host(), 120);
        self.embed_card("icons/zoom.svg")
            .debug_selector(move || format!("chat-remote-{index}"))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().truncate().child(if alt.is_empty() {
                        "A picture on the web".to_owned()
                    } else {
                        alt.to_owned()
                    }))
                    .child(
                        div()
                            .text_size(px((self.chat_text_size() - 2.).max(10.)))
                            .text_color(rgb(theme.muted))
                            .truncate()
                            .child(format!("Loading it tells {host} this machine's address")),
                    ),
            )
            .child(
                div()
                    .id(("chat-load-picture", index))
                    .debug_selector(|| "chat-load-picture".into())
                    .flex_none()
                    .px(px(10.))
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded(px(corners::CONTROL))
                    .border_1()
                    .border_color(rgb(theme.active))
                    .text_size(px((self.chat_text_size() - 1.).max(10.)))
                    .cursor_pointer()
                    .hover(|button| button.bg(rgb(theme.active)))
                    .child(format!("Load from {host}"))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        if let Some(pane) = this.chats.pane_mut(&pane_id) {
                            pane.prepared.visuals.want(id, Source::Remote(url.clone()));
                            pane.remeasure(entry);
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// The caption of a picture on the agent's machine, which herdr-acp
    /// shows after the message.
    pub(super) fn render_chat_picture_caption(&self, alt: &str) -> AnyElement {
        let theme = &self.theme;
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px((self.chat_text_size() - 1.).max(10.)))
            .text_color(rgb(theme.muted))
            .child(
                svg()
                    .path("icons/zoom.svg")
                    .size(px(11.))
                    .text_color(rgb(theme.muted)),
            )
            .child(if alt.is_empty() {
                "Picture below".to_owned()
            } else {
                format!("{alt} (below)")
            })
            .into_any_element()
    }

    fn embed_card(&self, icon: &'static str) -> Div {
        let theme = &self.theme;
        div()
            .max_w(px(520.))
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(corners::CONTROL))
            .border_1()
            .border_color(rgb(theme.active))
            .bg(rgb(theme.surface))
            .child(
                svg()
                    .flex_none()
                    .path(icon)
                    .size(px(14.))
                    .text_color(rgb(theme.subtext())),
            )
    }
}
