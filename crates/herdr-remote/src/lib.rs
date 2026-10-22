//! Remote control: a daemon that serves this machine's `herdr-acp` chats to
//! paired phones over HTTPS, and the control protocol Herdr UI uses to run it.
#![doc = include_str!("../README.md")]

#[cfg(feature = "server")]
mod auth;
pub mod control;
mod error;
#[cfg(feature = "server")]
mod hub;
#[cfg(feature = "server")]
mod tls;
#[cfg(feature = "server")]
mod web;

pub use error::{Error, Result};
