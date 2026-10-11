//! Railway's free VMs as `cloud` devices. `ssh railway.new` needs no account
//! or token: the SSH key you present is the identity, and the same key reaches
//! the same VM. So a Railway device is an SSH key, either one the user already
//! has or one this app generated, saved with the device (its path and
//! fingerprint; never a copy of the key) and used for setup and every
//! reconnect.
//!
//! Setup is the only place that generates a key, creates the VM (a one-off
//! session on a pseudo-terminal, see `first_use`) or installs Herdr (after
//! approval). Reconnects do none of these: they read the saved identity and run
//! Herdr's bridge. Everything here blocks, so it runs on background workers.

mod connect;
mod error;
mod first_use;
mod identity;
mod keys;
mod refusal;
pub(crate) mod setup;
mod ssh;

pub(crate) use connect::connect;
pub use error::Error;
pub(crate) use keys::{KeyDirectory, NAME_RULE, valid_name};

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The `account` every Railway device saves: the service the key is presented
/// to. It names no credential; the key lives in the identity store.
pub(crate) const ACCOUNT: &str = "railway.new";
