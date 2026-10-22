//! Remote control: a daemon that serves this machine's `herdr-acp` chats to
//! paired phones over HTTPS, and the control protocol Herdr UI uses to run it.
#![doc = include_str!("../README.md")]

#[cfg(feature = "server")]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        unused_imports,
        reason = "the server wires the APNs sender into POST /api/apns and the notifier next"
    )
)]
#[cfg_attr(
    test,
    allow(
        dead_code,
        unused_imports,
        reason = "tests reach only part of the APNs sender until the server wires it"
    )
)]
mod apns;
#[cfg(feature = "server")]
mod auth;
pub mod control;
mod error;
#[cfg(feature = "server")]
mod hub;
#[cfg(feature = "server")]
mod push;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
mod tls;
#[cfg(feature = "server")]
mod web;

pub use error::{Error, Result};
