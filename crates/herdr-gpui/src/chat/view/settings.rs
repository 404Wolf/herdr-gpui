//! The session's settings in the chat header: a button per setting worth
//! having at hand (model, thinking level, mode), the menu each opens, the
//! rest behind one more button, and how full the context window is.

use super::{super::chats::ChatPane, Hues};
use crate::{HerdrWindow, chat::chats::Menu, config::corners, notifications::safe_text};
use gpui::{prelude::*, *};
use herdr_chat_protocol::{
    Command, ConfigOption, OptionCategory, OptionKind, OptionValue, Transcript, display,
};

/// The settings the header shows a button for, in order.
const AT_HAND: [OptionCategory; 3] = [
    OptionCategory::Model,
    OptionCategory::ThoughtLevel,
    OptionCategory::Mode,
];
/// The share of the context window past which the meter warns, then alarms.
const WARN: u8 = 75;
const ALARM: u8 = 90;
const METER: f32 = 48.;

/// The settings the header has no button for.
fn others(transcript: &Transcript) -> Vec<&ConfigOption> {
    transcript
        .options
        .iter()
        .filter(|option| {
            !AT_HAND.contains(&option.category)
                || transcript.option(option.category).map(|first| &first.id) != Some(&option.id)
        })
        .collect()
}

impl HerdrWindow {
    fn chat_header_button(&self, id: SharedString) -> Stateful<Div> {
        let theme = &self.theme;
        div()
            .id(id.clone())
            .debug_selector(move || id.to_string())
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.))
            .px(px(6.))
            .h(px(22.))
            .rounded(px(corners::CONTROL))
            .border_1()
            .border_color(rgb(theme.active))
            .cursor_pointer()
            .hover(|button| button.bg(rgb(theme.active)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn toggle_chat_menu(&mut self, pane_id: &str, menu: Menu, cx: &mut Context<Self>) {
        if let Some(pane) = self.chats.pane_mut(pane_id) {
            pane.menu = (pane.menu.as_ref() != Some(&menu)).then_some(menu);
        }
        cx.notify();
    }

    /// The header's setting buttons: one per setting at hand, the agent's
    /// older modes when it has no mode setting, then one for the rest.
    pub(super) fn render_chat_settings_buttons(
        &self,
        chat: &ChatPane,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let theme = &self.theme;
        let transcript = chat.session.transcript();
        let pane_id = chat.target.pane_id.clone();
        let chevron = || {
            svg()
                .path("icons/chevron-down.svg")
                .size(px(10.))
                .text_color(rgb(theme.subtext()))
        };
        let mut buttons = Vec::new();
        for category in AT_HAND {
            let Some(option) = transcript.option(category) else {
                continue;
            };
            let id = option.id.clone();
            let pane = pane_id.clone();
            let button = self
                .chat_header_button(format!("chat-setting-{}", option.id).into())
                .child(
                    div()
                        .text_color(rgb(theme.subtext()))
                        .child(safe_text(&option.name, 40)),
                )
                .child(
                    div()
                        .max_w(px(200.))
                        .truncate()
                        .child(safe_text(&option.current_name(), 80)),
                );
            buttons.push(match &option.kind {
                OptionKind::Toggle { current } => {
                    let value = OptionValue::Toggle { value: !current };
                    button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        let command = Command::SetOption {
                            id: id.clone(),
                            value: value.clone(),
                        };
                        this.chat_send(&pane, command, cx);
                    }))
                }
                OptionKind::Select { .. } => button.child(chevron()).on_click(cx.listener(
                    move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.toggle_chat_menu(&pane, Menu::Setting(id.clone()), cx);
                    },
                )),
            });
        }
        if transcript.option(OptionCategory::Mode).is_none()
            && let Some(modes) = &transcript.modes
        {
            let name = modes
                .current()
                .map_or_else(|| modes.current.clone(), |mode| mode.name.clone());
            let pane = pane_id.clone();
            buttons.push(
                self.chat_header_button("chat-mode".into())
                    .child(div().max_w(px(160.)).truncate().child(safe_text(&name, 80)))
                    .child(chevron())
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.toggle_chat_menu(&pane, Menu::Modes, cx);
                    })),
            );
        }
        if !others(transcript).is_empty() {
            let pane = pane_id.clone();
            buttons.push(
                self.chat_header_button("chat-more-settings".into())
                    .child(
                        svg()
                            .path("icons/settings.svg")
                            .size(px(12.))
                            .text_color(rgb(theme.foreground)),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        this.toggle_chat_menu(&pane, Menu::Settings, cx);
                    })),
            );
        }
        buttons
    }

    /// How full the context window is, with what the session has cost.
    pub(super) fn render_chat_usage(&self, chat: &ChatPane) -> Option<Div> {
        let theme = &self.theme;
        let hues = Hues::of(theme);
        let usage = chat
            .session
            .transcript()
            .usage
            .as_ref()
            .filter(|usage| usage.size > 0)?;
        let percent = usage.percent();
        let color = match percent {
            ALARM.. => hues.red,
            WARN.. => hues.yellow,
            _ => hues.green,
        };
        let filled = METER * f32::from(percent) / 100.;
        let mut text = format!(
            "{} / {}",
            display::tokens(usage.used),
            display::tokens(usage.size)
        );
        if let Some(cost) = usage.cost.as_ref().filter(|cost| cost.micros > 0) {
            text.push_str(&format!(" \u{b7} {}", display::cost(cost)));
        }
        Some(
            div()
                .debug_selector(|| "chat-usage".into())
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_color(rgb(theme.subtext()))
                .child(
                    div()
                        .w(px(METER))
                        .h(px(4.))
                        .rounded_full()
                        .bg(rgb(theme.active))
                        .child(div().w(px(filled)).h_full().rounded_full().bg(rgb(color))),
                )
                .child(text),
        )
    }

    /// The menu `menu` opens, over the top of the chat.
    pub(super) fn render_chat_menu(
        &self,
        chat: &ChatPane,
        menu: &Menu,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = &self.theme;
        let transcript = chat.session.transcript();
        let pane_id = chat.target.pane_id.clone();
        let row = |index: usize, current: bool| {
            div()
                .id(("chat-menu-row", index))
                .debug_selector(move || format!("chat-menu-row-{index}"))
                .flex()
                .flex_col()
                .px(px(8.))
                .py(px(4.))
                .rounded(px(corners::SMALL))
                .cursor_pointer()
                .when(current, |row| row.bg(rgb(theme.active)))
                .hover(|row| row.bg(rgb(theme.active)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let heading = |text: &str| {
            div()
                .px(px(8.))
                .pt(px(6.))
                .pb(px(2.))
                .text_size(px((self.config.ui.size - 1.).max(10.)))
                .text_color(rgb(theme.muted))
                .child(safe_text(text, 80))
        };
        let described = |name: &str, description: Option<&String>| {
            (
                div().child(safe_text(name, 80)),
                description.map(|description| {
                    div()
                        .text_color(rgb(theme.subtext()))
                        .child(safe_text(description, 200))
                }),
            )
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        match menu {
            Menu::Modes => {
                let Some(modes) = &transcript.modes else {
                    return div().into_any_element();
                };
                for (index, mode) in modes.available.iter().enumerate() {
                    let (name, description) = described(&mode.name, mode.description.as_ref());
                    let pane = pane_id.clone();
                    let command = Command::SetMode {
                        id: mode.id.clone(),
                    };
                    rows.push(
                        row(index, mode.id == modes.current)
                            .child(name)
                            .children(description)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.toggle_chat_menu(&pane, Menu::Modes, cx);
                                this.chat_send(&pane, command.clone(), cx);
                            }))
                            .into_any_element(),
                    );
                }
            }
            Menu::Setting(id) => {
                let Some(option) = transcript.options.iter().find(|option| &option.id == id) else {
                    return div().into_any_element();
                };
                let OptionKind::Select { choices, current } = &option.kind else {
                    return div().into_any_element();
                };
                rows.push(heading(&option.name).into_any_element());
                let mut group = None;
                for (index, choice) in choices.iter().enumerate() {
                    if choice.group.is_some() && choice.group != group {
                        group.clone_from(&choice.group);
                        rows.push(
                            heading(choice.group.as_deref().unwrap_or("")).into_any_element(),
                        );
                    }
                    let (name, description) = described(&choice.name, choice.description.as_ref());
                    let pane = pane_id.clone();
                    let menu = menu.clone();
                    let command = Command::SetOption {
                        id: option.id.clone(),
                        value: OptionValue::Choice {
                            id: choice.id.clone(),
                        },
                    };
                    rows.push(
                        row(index, &choice.id == current)
                            .child(name)
                            .children(description)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.toggle_chat_menu(&pane, menu.clone(), cx);
                                this.chat_send(&pane, command.clone(), cx);
                            }))
                            .into_any_element(),
                    );
                }
            }
            Menu::Settings => {
                for (index, option) in others(transcript).into_iter().enumerate() {
                    let pane = pane_id.clone();
                    let id = option.id.clone();
                    let value = match &option.kind {
                        OptionKind::Toggle { current } => {
                            Some(OptionValue::Toggle { value: !current })
                        }
                        OptionKind::Select { .. } => None,
                    };
                    rows.push(
                        row(index, false)
                            .child(
                                div()
                                    .flex()
                                    .gap(px(8.))
                                    .child(div().flex_1().child(safe_text(&option.name, 80)))
                                    .child(
                                        div()
                                            .text_color(rgb(theme.subtext()))
                                            .child(safe_text(&option.current_name(), 80)),
                                    ),
                            )
                            .children(option.description.as_ref().map(|description| {
                                div()
                                    .text_color(rgb(theme.subtext()))
                                    .child(safe_text(description, 200))
                            }))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                match &value {
                                    Some(value) => {
                                        let command = Command::SetOption {
                                            id: id.clone(),
                                            value: value.clone(),
                                        };
                                        this.chat_send(&pane, command, cx);
                                    }
                                    None => {
                                        this.toggle_chat_menu(&pane, Menu::Setting(id.clone()), cx)
                                    }
                                }
                            }))
                            .into_any_element(),
                    );
                }
            }
        }
        div()
            .absolute()
            .top(px(66.))
            .left(px(8.))
            .w(px(300.))
            .child(
                div()
                    .id("chat-menu")
                    .debug_selector(|| "chat-menu".into())
                    .occlude()
                    .flex()
                    .flex_col()
                    .p(px(4.))
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .rounded(px(corners::CONTROL))
                    .border_1()
                    .border_color(rgb(theme.active))
                    .bg(rgb(theme.surface))
                    .shadow_md()
                    .children(rows),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
