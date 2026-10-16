//! Optional behaviors under `[features]`. Each flag is declared once below,
//! which generates its config field, its `Feature` variant, and the label and
//! description Settings > General lists it with.
use serde::Deserialize;

macro_rules! features {
    ($($field:ident: $variant:ident, $label:literal, $description:literal;)+) => {
        /// Optional behaviors the config file turns on. Every flag is off by
        /// default, so a missing or empty `[features]` table is the shipped
        /// experience.
        #[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
        #[serde(default)]
        pub struct Features {
            $(#[doc = $description] pub $field: bool,)+
        }

        /// One `[features]` flag, for code that lists or edits them all.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Feature {
            $($variant,)+
        }

        impl Feature {
            /// Every flag, alphabetical by key, as Settings lists them.
            pub const ALL: &[Self] = &[$(Self::$variant,)+];

            /// The key under `[features]`.
            pub fn key(self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($field),)+
                }
            }

            pub fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)+
                }
            }

            pub fn description(self) -> &'static str {
                match self {
                    $(Self::$variant => $description,)+
                }
            }
        }

        impl Features {
            pub fn enabled(&self, feature: Feature) -> bool {
                match feature {
                    $(Feature::$variant => self.$field,)+
                }
            }
        }
    };
}

// Settings lists the flags in this order, alphabetical by key, so additions
// from parallel branches land on different lines.
features! {
    // keep-sorted start
    sidebar_hover_menu: SidebarHoverMenu, "Open space menus on hover",
        "Open a space's menu when the pointer rests on its sidebar row, without a right click. Moving the pointer anywhere but into that menu closes it again.";
    // keep-sorted end
}
