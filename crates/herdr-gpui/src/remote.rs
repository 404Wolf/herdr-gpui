//! Remote control, Herdr UI's side: the `herdr-remote` daemon serves this
//! machine's chats to paired phones, and Herdr UI installs it as a user
//! service, watches it, pairs devices, and revokes them. Only the daemon's
//! control protocol is used here (`herdr_remote::control`), from background
//! threads; Tokio never reaches the app.
//!
//! [`service`] writes and loads the service, [`supervisor`] keeps it in line
//! with `[remote]` and polls its status, [`pairing`] and [`qr`] are the
//! pairing dialog's state and drawing, and [`dialog`], [`revoke`], and
//! [`indicator`] put them in the window.

mod dialog;
mod error;
mod indicator;
pub(crate) mod pairing;
pub(crate) mod qr;
mod revoke;
pub(crate) mod service;
pub(crate) mod supervisor;

pub(crate) use dialog::{PairingDialog, addresses};
pub use error::Error;
pub(crate) use error::Result;
pub(crate) use revoke::Revoke;
pub(crate) use supervisor::{Health, Supervisor};

use std::path::PathBuf;

/// The daemon's private state directory: the app's own, under `remote/`.
pub(crate) fn state_dir() -> Result<PathBuf> {
    crate::preferences::state_dir()
        .map(|dir| dir.join("remote"))
        .ok_or(Error::NoStateDir)
}

/// Where this user's service goes and what it runs. Reads the environment
/// and looks for the daemon on disk: run it off the UI thread.
fn host(state_dir: PathBuf) -> crate::Result<service::Host> {
    let manager = service::Manager::current()?;
    let home = crate::config::home()?;
    let config_home = herdr_client::local_var("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    Ok(service::Host {
        file: manager.file(&home, &config_home, &state_dir),
        manager,
        state_dir,
        program: crate::bundled::locate(service::PROGRAM),
    })
}

/// "Last seen 3 h ago" and the like, from milliseconds since the epoch.
pub(crate) fn ago(then_ms: u64, now_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(then_ms) / 1000;
    match seconds {
        0..60 => "just now".to_owned(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        _ => match seconds / 86_400 {
            1 => "yesterday".to_owned(),
            days => format!("{days} days ago"),
        },
    }
}

/// Milliseconds since the Unix epoch, now.
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests;
