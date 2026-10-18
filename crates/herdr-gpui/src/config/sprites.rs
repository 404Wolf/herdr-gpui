//! The `[sprites]` table: a Fly Sprites account whose sprites become devices.
//! A build without the `sprites` feature still parses the table, so one
//! config file serves every build, but never reads it. The API token is never
//! read from here: Settings keeps it in the credential store.
#[cfg(feature = "sprites")]
use super::Config;
#[cfg(feature = "sprites")]
use crate::Result;
use serde::Deserialize;
#[cfg(feature = "sprites")]
use std::{env, ffi::OsString, path::Path};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpritesConfig {
    // keep-sorted start
    pub allow_plaintext_credentials: bool,
    /// The API root, `https://api.sprites.dev` for Fly's service.
    pub api_url: Option<String>,
    // keep-sorted end
}

#[cfg(feature = "sprites")]
impl SpritesConfig {
    /// The validated account settings, or `None` when Sprites is not set up.
    /// `HERDR_SPRITES_API_URL` replaces `api_url`.
    pub(crate) fn settings(&self) -> Result<Option<crate::sprites::Settings>> {
        self.settings_with(|name| env::var_os(name))
    }

    pub(crate) fn settings_with(
        &self,
        var: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Option<crate::sprites::Settings>> {
        Ok(crate::sprites::Settings::resolve(self, var)?)
    }
}

/// The `[sprites]` keys the Settings window edits. An empty value removes its
/// key.
#[cfg(feature = "sprites")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SpritesFields {
    pub(crate) api_url: String,
}

#[cfg(feature = "sprites")]
impl SpritesFields {
    pub(crate) fn from_config(config: &SpritesConfig) -> Self {
        Self {
            api_url: config.api_url.clone().unwrap_or_default(),
        }
    }

    fn entries(&self) -> [(&'static str, &str); 1] {
        [("api_url", &self.api_url)]
    }

    /// The checks loading applies, so a save cannot leave a config that then
    /// fails to load.
    fn validate(&self, existing: &SpritesConfig) -> Result<()> {
        let api_url = self.api_url.trim();
        SpritesConfig {
            api_url: (!api_url.is_empty()).then(|| api_url.to_owned()),
            ..existing.clone()
        }
        .settings_with(|_| None)
        .map(drop)
    }
}

#[cfg(feature = "sprites")]
impl Config {
    /// Write the `[sprites]` keys Settings edits to the local override file.
    pub(crate) fn save_sprites(fields: &SpritesFields, existing: &SpritesConfig) -> Result<()> {
        fields.validate(existing)?;
        let (_lock, local) = Self::prepare_files(&Self::path()?)?;
        Self::save_sprites_path(fields, &local)
    }

    fn save_sprites_path(fields: &SpritesFields, path: &Path) -> Result<()> {
        super::table::save_keys(path, "sprites", &fields.entries())
    }
}

#[cfg(all(test, feature = "sprites"))]
mod tests;
