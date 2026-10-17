//! Form pieces for the pages that hold forms rather than setting rows (Code,
//! Plugins, Cloud devices): a titled block under the shared section heading,
//! and a labelled switch row. Both follow the row system's look.
use super::*;
use crate::settings_window::ui;

impl SettingsWindow {
    /// A block of form content under a section heading; no card frame.
    pub(in crate::settings_window) fn control_card(&self, title: &'static str) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .min_w_0()
            .child(ui::section(&self.theme, title, None))
    }

    /// A clickable row with `label` and a switch; the caller adds `.on_click`
    /// when `enabled`.
    pub(in crate::settings_window) fn control_switch(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        checked: bool,
        enabled: bool,
    ) -> Stateful<Div> {
        let id = id.into();
        let toggle = format!("{id}-toggle");
        div()
            .id(ElementId::Name(id.clone()))
            .debug_selector(move || id.to_string())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .min_h(px(ui::ROW_HEIGHT))
            .py(px(6.))
            .border_b_1()
            .border_color(rgb(self.theme.active))
            .when(enabled, |row| row.cursor_pointer())
            .when(!enabled, |row| row.opacity(0.5))
            .child(
                div()
                    .min_w_0()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(label.into()),
            )
            .child(ui::toggle(&self.theme, toggle, checked, true))
    }
}
