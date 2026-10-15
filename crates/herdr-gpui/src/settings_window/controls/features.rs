//! Settings > Features: one switch per `[features]` flag, each saved to this
//! client's local config like any other preference.
use super::*;
use crate::config::{Feature, preferences::Preference};

impl SettingsWindow {
    pub(super) fn render_feature_controls(&self, cx: &mut Context<Self>) -> Div {
        let features = self.config.features;
        Feature::ALL
            .iter()
            .fold(self.control_card("Optional features"), |card, &feature| {
                let on = features.enabled(feature);
                card.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            self.control_switch(
                                format!("settings-feature-{}", feature.key()),
                                feature.label(),
                                on,
                                true,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.save_preference(Preference::Feature(feature, !on), cx);
                                },
                            )),
                        )
                        .child(self.control_note(feature.description())),
                )
            })
    }
}
