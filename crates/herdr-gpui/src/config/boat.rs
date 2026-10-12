//! The `[boat]` table: a Boat account whose sandboxes become devices. A build
//! without the `boat` feature still parses the table, so one config file serves
//! every build, but never reads it. The API key is kept in the credential store.
#[cfg(feature = "boat")]
use super::Config;
#[cfg(feature = "boat")]
use crate::Result;
use serde::Deserialize;
#[cfg(feature = "boat")]
use std::{env, ffi::OsString, path::Path};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BoatConfig {
    /// The API root, `https://boat.dev/api/v1` for Boat's cloud.
    pub api_url: Option<String>,
    /// An organization ID or name; the active wallet if unset or empty.
    pub organization: Option<String>,
    /// `small`, `default`, `large`, or `xlarge`; Boat's default if unset or empty.
    pub machine_type: Option<String>,
    /// A named Boat environment; the account's default if unset or empty.
    pub environment: Option<String>,
    pub allow_plaintext_credentials: bool,
}

#[cfg(feature = "boat")]
impl BoatConfig {
    /// The validated account settings, or `None` when Boat is not set up.
    /// `HERDR_BOAT_API_URL` replaces `api_url`.
    pub(crate) fn settings(&self) -> Result<Option<crate::boat::Settings>> {
        self.settings_with(|name| env::var_os(name))
    }

    pub(crate) fn settings_with(
        &self,
        var: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Option<crate::boat::Settings>> {
        Ok(crate::boat::Settings::resolve(self, var)?)
    }
}

/// The `[boat]` keys the Settings window edits. An empty value removes its key.
#[cfg(feature = "boat")]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct BoatFields {
    pub(crate) api_url: String,
    pub(crate) organization: String,
    pub(crate) machine_type: String,
    pub(crate) environment: String,
}

#[cfg(feature = "boat")]
impl BoatFields {
    pub(crate) fn from_config(config: &BoatConfig) -> Self {
        let text = |value: &Option<String>| value.clone().unwrap_or_default();
        Self {
            api_url: text(&config.api_url),
            organization: text(&config.organization),
            machine_type: text(&config.machine_type),
            environment: text(&config.environment),
        }
    }

    fn entries(&self) -> [(&'static str, &str); 4] {
        [
            ("api_url", &self.api_url),
            ("organization", &self.organization),
            ("machine_type", &self.machine_type),
            ("environment", &self.environment),
        ]
    }

    fn validate(&self, existing: &BoatConfig) -> Result<()> {
        let value = |text: &str| {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_owned())
        };
        let config = BoatConfig {
            api_url: value(&self.api_url),
            organization: value(&self.organization),
            machine_type: value(&self.machine_type),
            environment: value(&self.environment),
            ..existing.clone()
        };
        config.settings_with(|_| None).map(drop)
    }
}

#[cfg(feature = "boat")]
impl Config {
    /// Write the `[boat]` keys Settings edits to the local override file.
    pub(crate) fn save_boat(fields: &BoatFields, existing: &BoatConfig) -> Result<()> {
        fields.validate(existing)?;
        let (_lock, local) = Self::prepare_files(&Self::path()?)?;
        Self::save_boat_path(fields, &local)
    }

    fn save_boat_path(fields: &BoatFields, path: &Path) -> Result<()> {
        super::table::save_keys(path, "boat", &fields.entries())
    }
}

#[cfg(all(test, feature = "boat"))]
mod tests;
