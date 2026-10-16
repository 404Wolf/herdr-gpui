//! Settings > Indicators: Herdr's shared dots or symbols, or one of this
//! client's drawn icon sets, which only Herdr UI shows.
use super::*;
use crate::config::{StatusIcons, preferences::Preference};
use herdr_client::protocol::AgentStatus;

const STATUSES: [(AgentStatus, &str, &str); 5] = [
    (AgentStatus::Working, "Working", "\u{25d0}"),
    (AgentStatus::Blocked, "Blocked", "\u{d7}"),
    (AgentStatus::Done, "Done", "\u{2713}"),
    (AgentStatus::Idle, "Idle", "\u{25cb}"),
    (AgentStatus::Unknown, "Unknown", "\u{b7}"),
];

impl SettingsWindow {
    pub(super) fn render_indicator_controls(&self, cx: &mut Context<Self>) -> Div {
        let mut card = self.control_card("Agent status indicators");
        let ready = self.controls_shared_ready();
        let light = matches!(
            cx.window_appearance(),
            WindowAppearance::Light | WindowAppearance::VibrantLight
        );
        let icons = self.config.status_icons;
        // Herdr's style as it applies now; Dots when its file is unreadable.
        let herdr = self
            .shared
            .as_ref()
            .map_or(IndicatorStyle::Dots, |shared| shared.indicators);
        for (label, style) in [
            ("Dots", IndicatorStyle::Dots),
            ("Symbols", IndicatorStyle::Symbols),
        ] {
            let selected = icons.is_none() && self.shared.is_some() && herdr == style;
            // Going back to the style Herdr already uses only drops this
            // client's key, so it works where the shared file is read-only.
            let enabled = ready || (icons.is_some() && herdr == style);
            let id = format!("settings-indicators-{label}");
            let mut choice = self
                .control_choice(id.clone(), label, selected, enabled)
                .debug_selector(move || id)
                .flex()
                .flex_col()
                .gap(px(18.))
                .when(enabled, |choice| {
                    choice.on_click(
                        cx.listener(move |this, _, _, cx| this.choose_herdr_indicators(style, cx)),
                    )
                });
            if let Some(shared) = &self.shared {
                let mut preview = div().flex().flex_wrap().gap(px(20.));
                for (status, name, symbol) in STATUSES {
                    let color = rgb(self.theme.ink(shared.status_color(status, light)));
                    let mark = div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(self.config.ui.size))
                        .h(px(self.config.ui.line_height()))
                        .when(style == IndicatorStyle::Symbols, |mark| {
                            mark.text_color(color).child(symbol)
                        })
                        .when(style == IndicatorStyle::Dots, |mark| {
                            mark.child(
                                div()
                                    .size(px(if status == AgentStatus::Unknown {
                                        3.
                                    } else {
                                        7.
                                    }))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(color)
                                    .when(status != AgentStatus::Idle, |dot| dot.bg(color)),
                            )
                        });
                    preview = preview.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(mark)
                            .child(name),
                    );
                }
                choice = choice.child(preview);
            }
            card = card.child(choice);
        }
        for (label, choice_icons) in StatusIcons::ALL.map(|icons| (icon_label(icons), icons)) {
            let indicators = crate::sidebar::Indicators::new(
                self.shared.as_ref(),
                Some(choice_icons),
                light,
                &self.theme,
            );
            let mut preview = div().flex().flex_wrap().gap(px(20.));
            for (status, name, _) in STATUSES {
                preview = preview.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(crate::sidebar::status_indicator(
                            status,
                            &self.config.ui,
                            indicators,
                        ))
                        .child(name),
                );
            }
            let id = format!("settings-indicators-{}", choice_icons.as_str());
            card = card.child(
                self.control_choice(id.clone(), label, icons == Some(choice_icons), true)
                    .debug_selector(move || id)
                    .flex()
                    .flex_col()
                    .gap(px(18.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.config.status_icons != Some(choice_icons) {
                            this.save_preference(Preference::StatusIcons(Some(choice_icons)), cx);
                        }
                    }))
                    .child(preview),
            );
        }
        card.child(self.control_note(
            "Solid, Tiles, and Edge rail are drawn by Herdr UI only and saved in its own \
             config; Herdr's terminal client keeps its dots or symbols.",
        ))
    }

    /// Picks Herdr's shared style, dropping this client's icons first so the
    /// choice shows. Both writes run as one save, the local one first: a
    /// failed shared write then still leaves Herdr's style in charge. The
    /// shared file is written only when it is editable and says otherwise.
    fn choose_herdr_indicators(&mut self, style: IndicatorStyle, cx: &mut Context<Self>) {
        if self.config.status_icons.is_none() {
            self.save_shared(Edit::Indicators(style), cx);
            return;
        }
        let shared = self
            .shared
            .clone()
            .filter(|shared| self.controls_shared_ready() && shared.indicators != style);
        let changed = shared.is_some();
        let shared_save = move || -> crate::Result<()> {
            if let Some(shared) = shared {
                shared.save(Edit::Indicators(style))?;
            }
            Ok(())
        };
        #[cfg(test)]
        if let Some(io) = self.controls.preference_io.clone() {
            self.save_with(
                move || {
                    (io.write)(Preference::StatusIcons(None))?;
                    shared_save()
                },
                io.load,
                changed,
                cx,
            );
            return;
        }
        self.save_with(
            move || {
                Config::save_preference(Preference::StatusIcons(None))?;
                shared_save()
            },
            self.loader(cx),
            changed,
            cx,
        );
    }
}

fn icon_label(icons: StatusIcons) -> &'static str {
    match icons {
        StatusIcons::Solid => "Solid",
        StatusIcons::Tiles => "Tiles",
        StatusIcons::Rail => "Edge rail",
    }
}
