//! Reaching a saved Railway device: read its saved identity, check the key is
//! still that identity, and run Herdr's stdio bridge through `ssh railway.new`.
//! Runs on the connection worker, and the UI retries it on its own, so this
//! must be safe to repeat: it never generates a key, never creates a VM or a
//! terminal session, never installs anything, and never falls back to another
//! identity. Making the VM ready is setup's job alone.

use super::{ACCOUNT, Error, Result, identity, keys, ssh};
use herdr_client::{Bridge, Transport};
use std::{
    io,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

fn connect_saved(dir: &Path, id: &str, session: &str, stop: &AtomicBool) -> Result<Bridge> {
    let cancelled = || stop.load(Ordering::Acquire);
    let record = identity::load(dir, id)?.ok_or(Error::NoIdentity)?;
    let key = record.key_path();
    if !key.is_file() {
        return Err(Error::KeyMissing(key));
    }
    // The file at the saved path must still be this device's key: a replaced
    // file or retargeted link would otherwise reach another VM.
    if keys::fingerprint(&key, &cancelled)? != record.id {
        return Err(Error::KeyChanged(key));
    }
    let config = ssh::Config::new(&key, &ssh::known_hosts(dir))?;
    // The bridge has started once this returns, so `ssh` no longer needs the
    // config file `config` removes when it drops.
    herdr_client::connect_command(config.command(), session, stop).map_err(Error::Bridge)
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
    match result {
        Ok(bridge) => Ok(bridge.into()),
        Err(Error::Bridge(error)) => Err(io::Error::new(error.kind(), error)),
        Err(error) => Err(io::Error::other(error)),
    }
}

// Tests drive `ssh`, `ssh-keygen`, and `/bin/sh`; cloud devices are Unix-only.
#[cfg(all(test, unix))]
mod tests;
