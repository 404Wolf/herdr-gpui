//! Asks before opening a terminal hyperlink whose scheme is not one that
//! opens directly. Terminal output is untrusted, and such an address starts
//! whatever application registered its scheme, so the user sees the whole
//! address first and nothing opens unless they say so.

use super::Page;
use crate::{HerdrWindow, fonts::StyledFont, terminal::ExternalUrl};
use gpui::{prelude::*, *};

impl HerdrWindow {
    /// Shows the question for `url`, unless another page holds the window.
    pub(crate) fn ask_to_open_link(
        &mut self,
        url: ExternalUrl,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.menu.page.is_some() || !self.open_menu(window, cx) {
            return;
        }
        self.menu.open_link = Some(url);
        self.menu.page = Some(Page::OpenLink);
    }

    /// Opens the address the question is about, and closes the question.
    pub(in crate::menu) fn confirm_open_link(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let url = self.menu.open_link.take();
        self.dismiss_menu(window, cx);
        if let Some(url) = url {
            cx.open_url(url.as_str());
        }
    }

    pub(in crate::menu) fn render_open_link(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let Some(url) = &self.menu.open_link else {
            return div();
        };
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .p(px(8.))
                .rounded(px(crate::config::corners::CONTROL))
                .when(primary, |button| button.bg(rgb(theme.active)))
                .when(!primary, |button| button.hover(|s| s.bg(rgb(theme.active))))
                .cursor_pointer()
                .child(label)
        };
        div()
            .debug_selector(|| "open-link".into())
            .child(
                div()
                    .p(px(8.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Are you sure you want to open this link?"),
            )
            .child(
                div().px(px(8.)).child(
                    div()
                        .debug_selector(|| "open-link-url".into())
                        .rounded(px(crate::config::corners::CONTROL))
                        .bg(rgb(theme.active))
                        .px(px(10.))
                        .py(px(6.))
                        .overflow_hidden()
                        .text_font(&self.config.terminal)
                        .child(url.to_string()),
                ),
            )
            .child(
                div().p(px(8.)).text_color(rgb(theme.muted)).child(
                    "A program in the terminal printed it. It opens in another application.",
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .p(px(8.))
                    .child(button("open-link-yes", "Yes", true).on_click(cx.listener(
                        |this, _, window, cx| {
                            cx.stop_propagation();
                            this.confirm_open_link(window, cx);
                        },
                    )))
                    .child(button("open-link-no", "No", false).on_click(cx.listener(
                        |this, _, window, cx| {
                            cx.stop_propagation();
                            this.dismiss_menu(window, cx);
                        },
                    ))),
            )
    }
}
