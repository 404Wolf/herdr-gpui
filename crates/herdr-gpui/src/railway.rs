//! Railway's free VMs as `cloud` devices. `ssh railway.new` needs no account
//! or token: the SSH key you present is the identity, and the same key reaches
//! the same VM. So a Railway device is an SSH identity, either a key the user
//! already has (borrowed, never modified or deleted) or one this app generated
//! and owns, saved with the device and used for setup and every reconnect.
//!
//! Railway refuses a scripted first connection for a new key (exit 13), so the
//! VM is created by the user, in an interactive terminal of their own, with a
//! command this module shows; nothing here pretends to be a terminal. Setup is
//! the only place a key is generated or Herdr is installed (after approval),
//! and it reaches the VM headlessly. Reconnects do neither: they read the saved
//! identity, stop for good once the VM's build window has ended (the device is
//! switched off rather than redialled forever), and run Herdr's bridge.
//! Everything here blocks, so it runs on background workers.

mod connect;
mod error;
mod identity;
mod keys;
mod manifest;
pub(crate) mod setup;
mod ssh;

pub(crate) use connect::connect;
pub use error::Error;
#[cfg(test)]
pub(crate) use identity::Record;
pub(crate) use identity::{Source, now};
pub(crate) use keys::{KeyDirectory, NAME_RULE, valid_name};

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The `account` every Railway device saves: the service the key is presented
/// to. It names no credential; the key lives in the identity store.
pub(crate) const ACCOUNT: &str = "railway.new";

/// How long Railway documents a free VM's build window to last. It is only
/// ever used as an upper bound from the moment setup last reached the VM.
pub(crate) const WINDOW_SECONDS: u64 = 60 * 60;

// Tests drive `ssh`, `ssh-keygen`, and `/bin/sh`; cloud devices are Unix-only.
#[cfg(all(test, unix))]
mod tests;
