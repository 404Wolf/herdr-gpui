//! Reaching a saved Railway device: read its saved identity, check the key is
//! still that identity, refuse once the free VM's build window has ended, and run Herdr's stdio bridge through
//! `ssh railway.new`. Runs on the connection worker, and the UI retries it on
//! its own, so this must be safe to repeat: it never generates a key, never
//! installs anything, and never falls back to another identity. Making the VM
//! ready is setup's job alone.

use super::{ACCOUNT, Error, Result, identity, keys, ssh};
use herdr_client::{Bridge, Transport};
use std::{
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

/// Devices switched off in this process after a refusal retrying cannot fix,
/// so a catalog that could not be written still stops the redialling.
static SUSPENDED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn suspended(id: &str) -> bool {
    let list = SUSPENDED.lock().unwrap_or_else(|error| error.into_inner());
    list.iter().any(|suspended| suspended == id)
}

/// Let `id` connect again, once the user has set the device up again.
pub(super) fn resume(id: &str) {
    SUSPENDED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .retain(|suspended| suspended != id);
}

/// The key to connect with, once the saved identity says connecting is safe.
/// Reads only; nothing here can create a key or a VM. `same_key` says whether
/// the file at the saved path still has the fingerprint the device was set up
/// with, so a replaced file or retargeted link never reaches another VM.
fn key_to_use(
    dir: &Path,
    id: &str,
    now: u64,
    same_key: impl FnOnce(&Path, &str) -> Result<bool>,
) -> Result<PathBuf> {
    if suspended(id) {
        return Err(Error::Suspended);
    }
    let record = identity::load(dir, id)?.ok_or(Error::NoIdentity)?;
    let key = record.key_path(dir);
    if !key.is_file() {
        return Err(Error::KeyMissing(key));
    }
    // After the window the VM is gone or about to be; there is nothing to
    // reach, and redialling Railway every few seconds would not change that.
    if record.window_ended(now) {
        return Err(Error::WindowEnded);
    }
    if !same_key(&key, &record.id)? {
        return Err(Error::KeyChanged(key));
    }
    Ok(key)
}

fn connect_saved(dir: &Path, id: &str, session: &str, stop: &AtomicBool) -> Result<Bridge> {
    let cancelled = || stop.load(Ordering::Acquire);
    let key = key_to_use(dir, id, identity::now(), |key, id| {
        Ok(keys::fingerprint(key, &cancelled)? == id)
    })?;
    let config = ssh::Config::new(&key, &ssh::known_hosts(dir))?;
    // The bridge has started once this returns, so `ssh` no longer needs the
    // config file `config` removes when it drops.
    herdr_client::connect_command(config.command(), session, stop).map_err(Error::Bridge)
}

/// Whether a failure is one retrying cannot fix: the build window ended, the
/// key changed, or Railway refused in a way only the user can resolve. The
/// endpoint redials on its own, forever, so such a device is switched off
/// (until Reconnect in Settings) instead of failing the same way every few
/// seconds. Transient network failures keep the normal backoff.
fn permanent(error: &Error) -> bool {
    use herdr_client::{Error as Client, SshFailure};
    match error {
        Error::WindowEnded | Error::KeyChanged(_) => true,
        Error::Bridge(Client::SshRefused(failure)) => {
            failure.needs_user() || *failure == SshFailure::Other
        }
        _ => false,
    }
}

fn stop_retrying(id: &str) {
    SUSPENDED
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(id.to_owned());
    if let Err(error) = crate::cloud::set_enabled(crate::cloud::CloudProvider::Railway, id, false) {
        tracing::warn!(category = "railway_connect", %error, "Could not switch off a refused Railway device");
    }
}

/// Reach the Railway device `id` in `account` for `cloud::connect`.
pub(crate) fn connect(
    account: &str,
    id: &str,
    session: &str,
    stop: &AtomicBool,
) -> io::Result<Transport> {
    let result = if account == ACCOUNT {
        identity::state_dir().and_then(|dir| connect_saved(&dir, id, session, stop))
    } else {
        Err(Error::Account)
    };
    if result.as_ref().is_err_and(permanent) {
        stop_retrying(id);
    }
    match result {
        Ok(bridge) => Ok(bridge.into()),
        Err(Error::Bridge(error)) => Err(io::Error::new(error.kind(), error)),
        Err(error) => Err(io::Error::other(error)),
    }
}

// Tests drive `ssh`, `ssh-keygen`, and `/bin/sh`; cloud devices are Unix-only.
#[cfg(all(test, unix))]
mod tests;
