//! The validated `[container]` table. Parsed once at the boundary, so every
//! value later passed to the engine is a known-good argument.

use super::{Engine, Error, Result};
use crate::config::ContainerConfig;

/// The image a new container starts from when none is set: Debian with Git,
/// curl, and build tools, so Herdr installs and repositories build as they
/// are.
pub(crate) const DEFAULT_IMAGE: &str = "buildpack-deps:bookworm";
pub(crate) const DEFAULT_CPUS: &str = "4";
pub(crate) const DEFAULT_MEMORY: &str = "8g";
pub(crate) const DEFAULT_PIDS: u32 = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Settings {
    /// `None` finds Docker, then Podman, when a container is made.
    pub(crate) engine: Option<Engine>,
    pub(crate) image: String,
    pub(crate) cpus: String,
    pub(crate) memory: String,
    pub(crate) pids: u32,
}

impl Settings {
    pub(crate) fn resolve(config: &ContainerConfig) -> Result<Self> {
        let engine = match config.engine.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(key) => Some(Engine::from_key(key).ok_or(Error::Field("[container] engine"))?),
        };
        let image = text(&config.image).unwrap_or(DEFAULT_IMAGE).to_owned();
        if !image_reference(&image) {
            return Err(Error::Field("[container] image"));
        }
        let cpus = text(&config.cpus).unwrap_or(DEFAULT_CPUS).to_owned();
        if !cpus
            .parse::<f32>()
            .is_ok_and(|cpus| cpus > 0. && cpus <= 512.)
        {
            return Err(Error::Field("[container] cpus"));
        }
        let memory = text(&config.memory).unwrap_or(DEFAULT_MEMORY).to_owned();
        if !memory_size(&memory) {
            return Err(Error::Field("[container] memory"));
        }
        let pids = match text(&config.pids_limit) {
            None => DEFAULT_PIDS,
            Some(text) => text
                .parse()
                .ok()
                .filter(|pids| (64..=1 << 20).contains(pids))
                .ok_or(Error::Field("[container] pids_limit"))?,
        };
        Ok(Self {
            engine,
            image,
            cpus,
            memory,
            pids,
        })
    }
}

fn text(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// A reference such as `ubuntu:24.04`, `ghcr.io/org/image@sha256:…`: passed
/// as one argument, and never one the engine could read as an option.
pub(crate) fn image_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('-')
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/' | b':' | b'@')
        })
}

/// A size such as `512m` or `8g`, in the units both engines accept.
pub(crate) fn memory_size(value: &str) -> bool {
    let digits = value.trim_end_matches(['b', 'k', 'm', 'g', 'B', 'K', 'M', 'G']);
    value.len() - digits.len() <= 1
        && !digits.is_empty()
        && digits.len() <= 12
        && digits.bytes().all(|b| b.is_ascii_digit())
        && digits.parse::<u64>().is_ok_and(|size| size > 0)
}
