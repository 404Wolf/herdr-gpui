//! The `[features]` flags in Settings > General: one switch per flag, each
//! with its description, saved to this client's local config like any other
//! preference. A flag added to the table shows up here on its own.
use super::*;
use crate::config::{Feature, preferences::Preference};

impl SettingsWindow {
    pub(super) fn feature_switches(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let features = self.config.features;
        Feature::ALL
            .iter()
            .map(|&feature| {
                let on = features.enabled(feature);
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
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.save_preference(Preference::Feature(feature, !on), cx);
                        })),
                    )
                    .child(self.control_note(feature.description()))
            })
            .collect()
    }
}
