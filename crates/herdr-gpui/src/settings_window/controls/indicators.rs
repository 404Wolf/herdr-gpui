//! Agent status indicators: Herdr's shared dots or symbols, or one of this
//! client's drawn icon sets, which only Herdr UI shows. The Notifications
//! page renders the choice; this keeps how a pick is saved.
use super::*;
use crate::{
    config::{StatusIcons, preferences::Preference},
    herdr_settings::{Edit, IndicatorStyle},
};

/// The name the Icons row gives one of Herdr UI's own icon sets.
pub(in crate::settings_window) fn icon_label(icons: StatusIcons) -> &'static str {
    match icons {
        StatusIcons::Solid => "Solid",
        StatusIcons::Tiles => "Tiles",
        StatusIcons::Rail => "Edge rail",
    }
}

impl SettingsWindow {
    /// Picks Herdr's shared style, dropping this client's icons first so the
    /// choice shows. Both writes run as one save, the local one first: a
    /// failed shared write then still leaves Herdr's style in charge. The
    /// shared file is written only when it is editable and says otherwise.
    pub(in crate::settings_window) fn choose_herdr_indicators(
        &mut self,
        style: IndicatorStyle,
        cx: &mut Context<Self>,
    ) {
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
            let load = io.load.clone();
            self.save_with(
                move || {
                    (io.write)(Preference::StatusIcons(None))?;
                    shared_save()
                },
                move || load(),
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

    /// Picks one of this client's icon sets, or none to follow Herdr's style.
    /// The set already in use saves nothing.
    pub(in crate::settings_window) fn choose_status_icons(
        &mut self,
        icons: Option<StatusIcons>,
        cx: &mut Context<Self>,
    ) {
        if self.config.status_icons != icons {
            self.save_preference(Preference::StatusIcons(icons), cx);
        }
    }
}
