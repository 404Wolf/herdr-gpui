//! Container failures keep their category and cause until the UI shows them.
//! Engine output is a bounded, control-free tail, never a credential.

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is not valid.")]
    Field(&'static str),
    #[error("Neither docker nor podman was found. Install one, or set [container] engine.")]
    NoEngine,
    #[error("Could not run {engine}.")]
    Spawn {
        engine: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("{engine} {action} failed: {detail}")]
    Failed {
        engine: &'static str,
        action: &'static str,
        detail: String,
    },
    #[error("{engine} {action} did not finish in time.")]
    Timeout {
        engine: &'static str,
        action: &'static str,
    },
    #[error("The container {0} no longer exists.")]
    Gone(String),
    #[error("A container or volume named {0} already exists; try again for another name.")]
    NameTaken(String),
    #[error("{0} on the engine was not made by Herdr UI for this device, so it was left alone.")]
    NotOurs(String),
    #[error(
        "There are already {0} containers, the most [container] max_containers allows. Remove one first."
    )]
    Limit(usize),
    #[error("Cancelled.")]
    Cancelled,
    #[error("{0}")]
    Bridge(#[source] herdr_client::Error),
    #[error(transparent)]
    Cloud(#[from] crate::cloud::Error),
    #[error("{0}")]
    Storage(#[source] Box<crate::Error>),
}
