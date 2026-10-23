//! Values fixed at build time or by the reference design: the product name the
//! OS sees, the tab strip's geometry, and the release identity the updater and
//! any credential store key off.

// Every window carries the product name; the focused space follows it.
pub(crate) const WINDOW_TITLE: &str = "Herdr";

// The bundle identifier and release version are the updater's, defined once
// beside the code that keys off them.
pub(crate) use herdr_updater::{APP_ID, APP_VERSION};

// Even tab cells, as on herdr.dev, so short labels do not collapse to a sliver.
pub(crate) const TAB_WIDTH: f32 = 64.;
// The reference strip is a shallow band: chrome, not a toolbar.
pub(crate) const TAB_HEIGHT: f32 = 24.;

// Only the signed release pipeline sets that tag. Local `just run` and worktree
// builds are unsigned, so features that depend on a stable code identity (such as
// macOS Keychain access) must treat them as development builds.
pub(crate) const RELEASE_BUILD: bool = option_env!("HERDR_RELEASE_VERSION").is_some();

// The git commit hash embedded by build.rs for source/dev builds.
pub(crate) const BUILD_COMMIT: Option<&str> = option_env!("HERDR_BUILD_COMMIT");
