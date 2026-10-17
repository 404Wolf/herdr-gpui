//! Notifications: delivery, in-app toasts, sound and status indicators.
use super::{SettingsWindow, TOAST_POSITIONS, on_off, on_value, ui};
use crate::settings_window::controls::indicators::icon_label;
use crate::settings_window::inputs::{DropdownField, NumberField};
use crate::{
    config::preferences::Preference,
    herdr_settings::{Edit, IndicatorStyle, ToastDelivery},
};
use gpui::{prelude::*, *};
use herdr_client::protocol::AgentStatus;

/// Whether a style segment responds: any style while Herdr's file is
/// writable, otherwise only the one Herdr already uses.
pub(in crate::settings_window) fn indicator_style_live(
    style: IndicatorStyle,
    herdr: IndicatorStyle,
    ready: bool,
) -> bool {
    ready || style == herdr
}

impl SettingsWindow {
    pub(in crate::settings_window) fn render_notifications(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let ready = self.controls_shared_ready();
        let shared = self.shared.as_ref();
        let overrides = self.config.notification_overrides;
        let shared_enabled = shared.is_some_and(|s| s.toast_delivery == ToastDelivery::Herdr);
        let position = overrides
            .position()
            .and_then(|p| TOAST_POSITIONS.iter().position(|(c, _)| *c == p));
        let position_names = TOAST_POSITIONS.map(|(_, name)| name);
        let base_position = shared.map_or(
            crate::config::NotificationConfig::default().position,
            |shared| shared.toast_position,
        );
        let base_position = TOAST_POSITIONS
            .iter()
            .find_map(|(p, name)| (*p == base_position).then_some(*name))
            .unwrap_or_default();
        let sound = shared.is_some_and(|shared| shared.sound_enabled);
        let finished = self.config.notifications.finished;
        let icons = self.config.status_icons;
        // Herdr's style as it applies now; Dots when its file is unreadable.
        let herdr = shared.map_or(IndicatorStyle::Dots, |shared| shared.indicators);
        let mut icon_choices = vec![(None, "Herdr")];
        icon_choices
            .extend(crate::config::StatusIcons::ALL.map(|icons| (Some(icons), icon_label(icons))));
        let source_alive = self.source.upgrade().is_some();
        let light = matches!(
            cx.window_appearance(),
            WindowAppearance::Light | WindowAppearance::VibrantLight
        );
        let styles = [
            (IndicatorStyle::Dots, "Dots"),
            (IndicatorStyle::Symbols, "Symbols"),
        ]
        .map(|(style, label)| {
            (
                style,
                self.indicator_segment(style, label, light)
                    .into_any_element(),
            )
        });
        div()
            .flex()
            .flex_col()
            .child(ui::section(theme, "Delivery", Some(ui::Source::Shared)))
            .child(
                self.gate_shared(
                    ui::row(theme, "notif-delivery", "Deliver agent notifications").description(
                        "Herdr shows them in-app. System uses OS notifications; clicking one \
                         opens its pane. Terminal is for other clients only.",
                    ),
                )
                .control(ui::segmented(
                    theme,
                    "notif-delivery-control",
                    &[
                        (ToastDelivery::Off, "Off"),
                        (ToastDelivery::Herdr, "Herdr"),
                        (ToastDelivery::Terminal, "Terminal"),
                        (ToastDelivery::System, "System"),
                    ],
                    shared.map(|shared| shared.toast_delivery),
                    ready,
                    on_value(cx, |this, mode, _, cx| {
                        this.save_shared(Edit::Toasts(mode), cx)
                    }),
                )),
            )
            .child(
                ui::row(theme, "notif-finished", "Finished notifications")
                    .tag(ui::Source::Local)
                    .description(
                        "Off drops the notice at the end of each agent turn, in-app and from \
                         the system; \u{201c}needs attention\u{201d} still shows.",
                    )
                    .control(self.preference_toggle(
                        "notif-finished-toggle",
                        finished,
                        Preference::NotificationFinished(!finished),
                        cx,
                    )),
            )
            .child(ui::section(theme, "In-app toasts", Some(ui::Source::Local)))
            .child(
                ui::row(theme, "notif-show", "Show toasts").control(self.override_dropdown(
                    DropdownField::ToastEnabled,
                    on_off(shared_enabled),
                    &["On", "Off"],
                    overrides.enabled().map(|on| usize::from(!on)),
                    cx,
                )),
            )
            .child(self.delay_row(cx))
            .child(
                ui::row(theme, "notif-position", "Position")
                    .description(
                        "Controls show effective values; a managed-file override still \
                         takes precedence.",
                    )
                    .control(self.override_dropdown(
                        DropdownField::ToastPosition,
                        base_position,
                        &position_names,
                        position,
                        cx,
                    )),
            )
            .child(ui::section(theme, "Sound", None))
            .child(
                self.gate_shared(
                    ui::row(theme, "notif-sound", "Play agent sounds")
                        .tag(ui::Source::Shared)
                        .description(if source_alive {
                            "Uses shared sound paths and per-agent overrides. Missing custom \
                             sounds fall back to bundled sounds."
                        } else {
                            "Uses shared sound paths and per-agent overrides. Missing custom \
                             sounds fall back to bundled sounds. Open a main Herdr window to \
                             preview audio."
                        }),
                )
                .control(
                    ui::button(theme, "notif-sound-test", "Test", source_alive).when(
                        source_alive,
                        |button| {
                            button.on_click(cx.listener(|this, _, _, cx| {
                                if let Some(source) = this.source.upgrade() {
                                    source.read(cx).sound.preview();
                                }
                            }))
                        },
                    ),
                )
                .control(
                    ui::toggle(theme, "notif-sound-toggle", sound, ready).when(ready, |toggle| {
                        toggle.on_click(cx.listener(move |this, _, _, cx| {
                            this.save_shared(Edit::Sound(!sound), cx)
                        }))
                    }),
                ),
            )
            .child(self.volume_row(cx))
            .child(ui::section(theme, "Status indicators", None))
            .child(
                self.gate_shared(
                    ui::row(theme, "notif-indicators", "Style")
                        .tag(ui::Source::Shared)
                        .description("How agent status shows in the sidebar."),
                )
                .control(ui::segmented_elements(
                    theme,
                    "notif-indicators-control",
                    styles.into(),
                    // Herdr's style shows only while no icon set replaces it.
                    shared
                        .filter(|_| icons.is_none())
                        .map(|shared| shared.indicators),
                    // Going back to the style Herdr already uses only drops
                    // this client's key, so it works where Herdr's file is
                    // read-only; the other styles stay dimmed there.
                    ready || icons.is_some(),
                    move |style| indicator_style_live(style, herdr, ready),
                    on_value(cx, move |this, style, _, cx| {
                        if indicator_style_live(style, herdr, ready) {
                            this.choose_herdr_indicators(style, cx)
                        }
                    }),
                )),
            )
            .child(
                ui::row(theme, "notif-icons", "Icons")
                    .tag(ui::Source::Local)
                    .description(
                        "Solid, Tiles and Edge rail are drawn by Herdr UI only; Herdr's \
                         terminal client keeps its dots or symbols.",
                    )
                    .control(ui::segmented(
                        theme,
                        "notif-icons-control",
                        &icon_choices,
                        Some(icons),
                        true,
                        on_value(cx, |this, icons, _, cx| this.choose_status_icons(icons, cx)),
                    )),
            )
    }

    /// The delay in seconds: a typed field with steppers, and a reset when the
    /// local file overrides Herdr's delay.
    fn delay_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let overridden = self.config.notification_overrides.delay_seconds().is_some();
        let on_change = on_value(cx, |this, change, _, cx| {
            this.commit_number(NumberField::ToastDelay, change, cx)
        });
        let value = self.config.notifications.delay_seconds as f32;
        let view = cx.entity().downgrade();
        ui::row(&self.theme, "notif-delay", "Delay")
            .description(
                "Wait before a toast appears, so quick turns stay quiet. Controls show \
                 effective values; a managed-file override still takes precedence.",
            )
            .control(ui::number(
                &self.theme,
                ui::Number {
                    id: NumberField::ToastDelay.selector(),
                    value,
                    range: NumberField::ToastDelay.range(),
                    unit: "s",
                    slider: false,
                    enabled: true,
                },
                self.number_edit_for(NumberField::ToastDelay),
                on_change,
                move |window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.begin_number_edit(NumberField::ToastDelay, value, window, cx)
                    });
                },
            ))
            .when(overridden, |row| {
                row.control(
                    ui::button(&self.theme, "notif-delay-reset", "Reset", true).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.save_preference(Preference::NotificationDelay(None), cx)
                        }),
                    ),
                )
            })
    }

    /// The local sound volume: a slider and a typed percentage, editable
    /// even where Herdr's shared sound settings are read-only, since it lives
    /// in this app's own file.
    fn volume_row(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let on_change = on_value(cx, |this, change, _, cx| {
            this.commit_number(NumberField::SoundVolume, change, cx)
        });
        let value = self.number_value(
            NumberField::SoundVolume,
            f32::from(self.config.sound.volume),
        );
        let view = cx.entity().downgrade();
        ui::row(&self.theme, "notif-volume", "Volume")
            .tag(ui::Source::Local)
            .description("Sounds and Test play at this level, even when sounds are off.")
            .control(ui::number(
                &self.theme,
                ui::Number {
                    id: NumberField::SoundVolume.selector(),
                    value,
                    range: NumberField::SoundVolume.range(),
                    unit: "%",
                    slider: true,
                    enabled: true,
                },
                self.number_edit_for(NumberField::SoundVolume),
                on_change,
                move |window, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.begin_number_edit(NumberField::SoundVolume, value, window, cx)
                    });
                },
            ))
    }

    /// A segment label: the style's name over the five status marks in their
    /// current colours.
    fn indicator_segment(&self, style: IndicatorStyle, label: &'static str, light: bool) -> Div {
        let mut marks = div().flex().items_center().gap(px(4.));
        if let Some(shared) = &self.shared {
            for (status, symbol) in [
                (AgentStatus::Working, "\u{25d0}"),
                (AgentStatus::Blocked, "\u{d7}"),
                (AgentStatus::Done, "\u{2713}"),
                (AgentStatus::Idle, "\u{25cb}"),
                (AgentStatus::Unknown, "\u{b7}"),
            ] {
                let color = rgb(self.theme.ink(shared.status_color(status, light)));
                marks = marks.child(
                    div()
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
                        }),
                );
            }
        }
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(label)
            .child(marks)
    }
}
