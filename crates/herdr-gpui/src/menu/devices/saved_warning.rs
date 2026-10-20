//! The completed Add Device state: the device is saved before this is shown.
//! Recovery advice wraps and stays available until the user closes the dialog.
use crate::HerdrWindow;
use gpui::{prelude::*, *};

impl HerdrWindow {
    pub(super) fn render_device_saved_warning(
        &self,
        warning: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("device-saved-warning-scroll")
                    .debug_selector(|| "device-saved-warning-scroll".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(16.))
                    .child(
                        div()
                            .debug_selector(|| "device-saved-warning".into())
                            .w_full()
                            .whitespace_normal()
                            .text_color(rgb(self.theme.ink(self.theme.palette[3])))
                            .child(warning),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .p(px(16.))
                    .border_t_1()
                    .border_color(rgb(self.theme.active))
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .id("device-saved-done")
                            .debug_selector(|| "device-saved-done".into())
                            .p(px(8.))
                            .rounded(px(crate::config::corners::CONTROL))
                            .bg(rgb(self.theme.active))
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(rgb(self.theme.active)
                                    .blend(rgba((self.theme.foreground << 8) | 0x20)))
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss_menu(window, cx);
                            }))
                            .child("Done"),
                    ),
            )
    }
}
