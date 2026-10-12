//! Settings and app services: config, themes, fonts, shortcuts, keeping the
//! machine awake, sounds, updates, and integrations.
use super::app::Switch;
use serde::{Deserialize, Serialize};

/// Listing installed fonts or integrations can take a few seconds.
const LIST_WAIT_SECONDS: u64 = 15;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SettingsOp {
    /// Reread Herdr's config and this app's, as the shortcut does.
    ReloadConfig,
    /// Unknown keys or missing fonts in the loaded config.
    CheckConfig,
    /// Choose and save a color theme.
    Theme {
        name: String,
    },
    /// The font families the app can use.
    Fonts,
    /// Choose and save the font of every face; none goes back to the default.
    Font {
        #[serde(default)]
        name: Option<String>,
    },
    /// The window's shortcuts, and whether each of Herdr's custom commands
    /// can be reached from this client.
    Keybinds,
    /// Report, or change for `seconds` (none: until changed again).
    Caffeine {
        #[serde(default)]
        switch: Option<Switch>,
        #[serde(default)]
        seconds: Option<u64>,
    },
    Sound {
        sound: Sound,
    },
    Bell,
    /// Ask for the latest release; never downloads or installs.
    CheckUpdate,
    Update,
    /// The integrations Herdr offers the selected host.
    Integrations,
    /// Install, or keep current, the agent skill.
    InstallSkill,
}

impl SettingsOp {
    pub(crate) fn wait_seconds(&self) -> u64 {
        match self {
            Self::Fonts | Self::Integrations | Self::CheckUpdate => LIST_WAIT_SECONDS,
            _ => 0,
        }
    }
}

/// The notification sounds Herdr names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sound {
    Done,
    Request,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BindingInfo {
    pub name: String,
    pub label: String,
    pub keys: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CustomInfo {
    pub label: String,
    pub keys: String,
    /// "runs", "shadowed", "taken", "needs_modifier", "no_prefix",
    /// "over_limit", or "unsupported".
    pub reach: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IntegrationInfo {
    pub id: String,
    pub label: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Reply {
    Config {
        diagnostic: Option<String>,
    },
    Fonts {
        fonts: Vec<String>,
    },
    Keybinds {
        bindings: Vec<BindingInfo>,
        custom: Vec<CustomInfo>,
    },
    /// "off", "pending", or "on".
    Caffeine {
        state: String,
    },
    Update {
        state: String,
        latest: Option<String>,
    },
    Integrations {
        integrations: Vec<IntegrationInfo>,
    },
}
