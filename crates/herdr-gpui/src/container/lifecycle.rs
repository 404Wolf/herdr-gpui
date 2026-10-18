//! Intentional stops are shared by every window and connection worker. Only
//! an explicit reconnect clears them. Engine operations serialize per device
//! so a start already in flight cannot finish after a successful stop.

use super::{Engine, Error, Result};
use herdr_client::{CloudProvider, ConnectTarget};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

#[derive(Default)]
struct Lifecycle {
    // Never hold this lock while running an engine command: the UI reads it.
    suspended: Mutex<Option<Arc<()>>>,
    operation: Mutex<()>,
}

type Devices = HashMap<String, HashMap<String, Arc<Lifecycle>>>;
static DEVICES: LazyLock<Mutex<Devices>> = LazyLock::new(Mutex::default);

fn device(account: &str, id: &str) -> Arc<Lifecycle> {
    DEVICES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(account.to_owned())
        .or_default()
        .entry(id.to_owned())
        .or_default()
        .clone()
}

fn target_device(target: &ConnectTarget) -> Option<Arc<Lifecycle>> {
    let ConnectTarget::Cloud {
        provider: CloudProvider::Container,
        account,
        id,
        ..
    } = target
    else {
        return None;
    };
    DEVICES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(account)?
        .get(id)
        .cloned()
}

pub(crate) fn suspended(target: &ConnectTarget) -> bool {
    target_device(target).is_some_and(|device| device.is_suspended())
}

/// UI-safe: releases the stop intent without waiting for the engine. The next
/// connection worker waits for any pending stop before starting the container.
pub(crate) fn resume(target: &ConnectTarget) {
    if let Some(device) = target_device(target) {
        device.resume();
    }
}

pub(super) fn start(engine: Engine, id: &str, start: impl FnOnce() -> Result<()>) -> Result<()> {
    device(engine.key(), id).start(id, start)
}

pub(crate) fn stop(engine: Engine, id: &str, stop: impl FnOnce() -> Result<()>) -> Result<()> {
    device(engine.key(), id).stop(stop)
}

impl Lifecycle {
    fn is_suspended(&self) -> bool {
        self.suspended
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    fn resume(&self) {
        self.suspended
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }

    fn start(&self, id: &str, start: impl FnOnce() -> Result<()>) -> Result<()> {
        let _operation = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.is_suspended() {
            return Err(Error::Suspended(id.to_owned()));
        }
        start()
    }

    fn stop(&self, stop: impl FnOnce() -> Result<()>) -> Result<()> {
        let intent = Arc::new(());
        *self
            .suspended
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(intent.clone());
        let _operation = self
            .operation
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = stop();
        if result.is_err() {
            let mut suspended = self
                .suspended
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // A failed old stop must not undo a newer stop or explicit resume.
            if suspended
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &intent))
            {
                suspended.take();
            }
        }
        result
    }
}

#[cfg(test)]
mod tests;
