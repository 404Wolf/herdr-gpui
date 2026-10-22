//! Remote control: a daemon that serves this machine's `herdr-acp` chats to
//! paired phones over HTTPS, and the control protocol Herdr UI uses to run it.
#![doc = include_str!("../README.md")]

pub mod control;
mod error;

pub use error::{Error, Result};
