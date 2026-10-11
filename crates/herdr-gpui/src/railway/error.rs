//! Railway failures keep their category and cause until the UI displays them.
//! Remote text is bounded diagnostic payload, never a credential.

use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Railway devices need ssh on the PATH; it could not be run.")]
    SshMissing(#[source] io::Error),
    #[error("Railway devices need ssh-keygen on the PATH; it could not be run.")]
    KeygenMissing(#[source] io::Error),
    #[error("ssh-keygen could not create or read the key (exit status {0:?}).")]
    Keygen(Option<i32>),
    #[error("ssh-keygen did not finish within 30 seconds.")]
    KeygenTimeout,
    #[error(
        "{} is not a usable SSH private key. A passphrase-protected key needs its .pub file next to it and must be loaded in your SSH agent.",
        .0.display()
    )]
    UnusableKey(PathBuf),
    #[error("Choose an absolute path to an SSH private key.")]
    KeyPath,
    #[error("Could not use the key file {}.", path.display())]
    KeyFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Could not update {}.", path.display())]
    Store {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Invalid saved Railway identities.")]
    Json(#[source] serde_json::Error),
    #[error("{0} is not valid.")]
    Invalid(&'static str),
    #[error("This device has no saved SSH identity. Remove it and set it up again.")]
    NoIdentity,
    #[error(
        "The SSH key for this device is missing: {}. It is never replaced with a new one; restore it, or remove the device.",
        .0.display()
    )]
    KeyMissing(PathBuf),
    #[error(
        "The key at {} is not the one this device was set up with, so it was not used. Restore that key, or remove the device and set it up again.",
        .0.display()
    )]
    KeyChanged(PathBuf),
    #[error(
        "This Railway free VM's build window has ended, so Herdr stopped reconnecting. Use Reconnect in Settings > Cloud Devices > Railway to check it again, or Mark claimed if you claimed it."
    )]
    WindowEnded,
    #[error(
        "Railway refused this device in a way retrying cannot fix, so it was switched off. Use Reconnect in Settings > Cloud Devices > Railway after fixing the cause."
    )]
    Suspended,
    #[error(
        "Railway only creates a VM for a new key from an interactive terminal; it refuses scripted first connections. Run this once yourself, exit the VM shell, then connect again: {command}"
    )]
    FirstUseRequired { command: String },
    #[error("Railway says this VM's build window has already ended, so it is not set up.")]
    Expired,
    #[error("Could not reach railway.new: {0}")]
    Unreachable(String),
    #[error("This device belongs to a different Railway service than railway.new.")]
    Account,
    #[error("Could not prepare the SSH command for Railway.")]
    ConfigFile(#[source] io::Error),
    #[error("{0}")]
    Bridge(#[source] herdr_client::Error),
    #[error(transparent)]
    Cloud(#[from] crate::cloud::Error),
}
