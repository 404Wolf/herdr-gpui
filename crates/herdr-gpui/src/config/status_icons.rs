//! `status_icons`: drawn status icons that replace Herdr's dots or symbols in
//! this client only. Herdr's `ui.status_indicators` accepts just those two
//! values and refuses the whole config on any other, so these styles live in
//! this app's config, and unsetting the key goes back to Herdr's choice.

use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StatusIcons {
    /// Filled discs for done and blocked, a ring for idle, half a disc for working.
    Solid,
    /// Each mark on a tile tinted with its status color.
    Tiles,
    /// Outline marks, plus a bar on the row's edge while an agent is working,
    /// done, or blocked.
    Rail,
}

impl StatusIcons {
    pub const ALL: [Self; 3] = [Self::Solid, Self::Tiles, Self::Rail];

    /// The config spelling, which deserializing reads back.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Tiles => "tiles",
            Self::Rail => "rail",
        }
    }
}
