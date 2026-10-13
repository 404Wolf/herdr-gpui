//! The terminal's colors per saved device (`[devices.<id>] theme`). Only the
//! terminal changes: the chrome keeps the app theme. Every terminal a window
//! paints belongs to its selected endpoint, parked groups included, so one
//! theme serves the whole terminal area.
use crate::{
    HerdrWindow,
    config::{Config, Theme},
    contrast::Contrast,
};
use gpui::{Context, Task};
use std::collections::BTreeMap;

/// What a set of resolved themes was read for.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    light: bool,
    contrast: Contrast,
    /// Each device's `theme` value, by profile ID, in profile order.
    themes: Vec<(String, String)>,
}

impl Key {
    fn matches(&self, config: &Config, light: bool) -> bool {
        self.light == light
            && self.contrast == config.contrast
            && self
                .themes
                .iter()
                .map(|(profile, theme)| (profile.as_str(), theme.as_str()))
                .eq(config.device_themes())
    }

    fn new(config: &Config, light: bool) -> Self {
        Self {
            light,
            contrast: config.contrast,
            themes: config
                .device_themes()
                .map(|(profile, theme)| (profile.to_owned(), theme.to_owned()))
                .collect(),
        }
    }
}

/// Device themes resolved off the UI thread, since a theme may be a file.
#[derive(Default)]
pub(crate) struct DeviceThemes {
    resolved: Option<Key>,
    themes: BTreeMap<String, Theme>,
    /// The key being read, and its task.
    load: Option<(Key, Task<()>)>,
    /// A choice just saved, shown until the config and its read catch up.
    pinned: Option<Pinned>,
}

/// A device theme saved from the menu. The save reloads the config and the
/// reload starts a read; without the pin the terminal would show the old
/// colors in between.
pub(crate) struct Pinned {
    pub(crate) endpoint: String,
    /// The saved value, `None` for the app theme.
    pub(crate) value: Option<String>,
    /// Its colors, `None` for the app theme.
    pub(crate) theme: Option<Theme>,
    /// `config_load_revision` when it was saved.
    pub(crate) revision: u64,
}

impl DeviceThemes {
    /// The device's resolved theme. One that failed to load has none, and
    /// the terminal keeps the app theme.
    fn get(&self, profile: &str) -> Option<&Theme> {
        self.themes.get(profile)
    }
}

/// Reads every theme in `key`. A theme that cannot be read is logged and
/// left out rather than failing the others.
fn resolve(config: &Config, key: &Key) -> BTreeMap<String, Theme> {
    key.themes
        .iter()
        .filter_map(|(profile, value)| match config.theme_named(value, key.light) {
            Ok(theme) => Some((profile.clone(), theme)),
            Err(error) => {
                tracing::warn!(profile, theme = value, %error, "Could not load the device theme");
                None
            }
        })
        .collect()
}

impl HerdrWindow {
    /// Brings the resolved device themes in line with the config and the
    /// system appearance. Called while rendering; it only compares until
    /// either changes, and the files are read on the background executor.
    pub(super) fn sync_device_themes(&mut self, cx: &mut Context<Self>) {
        let light = crate::app::light_appearance(cx);
        self.release_device_theme_pin(light);
        let state = &self.device_themes;
        if state
            .load
            .as_ref()
            .map(|(key, _)| key)
            .or(state.resolved.as_ref())
            .is_some_and(|key| key.matches(&self.config, light))
        {
            return;
        }
        let key = Key::new(&self.config, light);
        if key.themes.is_empty() {
            let state = &mut self.device_themes;
            state.resolved = Some(key);
            state.themes.clear();
            state.load = None;
            return;
        }
        let config = self.config.clone();
        let read = cx.background_executor().spawn({
            let key = key.clone();
            async move {
                let themes = resolve(&config, &key);
                (key, themes)
            }
        });
        let task = cx.spawn(async move |this, cx| {
            let (key, themes) = read.await;
            let _ = this.update(cx, |this, cx| {
                // A newer key replaced this read; its own result applies.
                if this
                    .device_themes
                    .load
                    .as_ref()
                    .is_none_or(|(loading, _)| *loading != key)
                {
                    return;
                }
                this.device_themes.resolved = Some(key);
                this.device_themes.themes = themes;
                this.device_themes.load = None;
                cx.notify();
            });
        });
        self.device_themes.load = Some((key, task));
    }

    /// Shows `pinned` until the config holds it and its colors are read.
    pub(crate) fn pin_device_theme(&mut self, pinned: Pinned) {
        self.device_themes.pinned = Some(pinned);
    }

    /// Drops the pin once a reload has landed: either the config now says
    /// what was saved and its read is done, or it says something else, as
    /// after a failed save, and the pin would only hide that.
    fn release_device_theme_pin(&mut self, light: bool) {
        let Some(pinned) = &self.device_themes.pinned else {
            return;
        };
        if pinned.revision == self.config_load_revision {
            return;
        }
        let saved = self.config.device_theme(&pinned.endpoint) == pinned.value.as_deref();
        let read = self
            .device_themes
            .resolved
            .as_ref()
            .is_some_and(|key| key.matches(&self.config, light));
        if !saved || read {
            self.device_themes.pinned = None;
        }
    }

    /// The colors the terminal area paints with: the selected device's own
    /// theme while it has one, else the app theme.
    pub(crate) fn terminal_theme(&self) -> &Theme {
        let Some(endpoint) = self.endpoints.get(self.selected_endpoint) else {
            return &self.theme;
        };
        if let Some(preview) = self.device_theme_preview(&endpoint.id) {
            return preview;
        }
        if let Some(pinned) = self
            .device_themes
            .pinned
            .as_ref()
            .filter(|pinned| pinned.endpoint == endpoint.id)
        {
            return pinned.theme.as_ref().unwrap_or(&self.theme);
        }
        // A theme removed from the config stops at once, without waiting
        // for the read that drops it.
        if self.config.device_theme(&endpoint.id).is_none() {
            return &self.theme;
        }
        crate::endpoint::saved_profile_id(&endpoint.id)
            .and_then(|profile| self.device_themes.get(profile))
            .unwrap_or(&self.theme)
    }
}

#[cfg(test)]
mod tests;
