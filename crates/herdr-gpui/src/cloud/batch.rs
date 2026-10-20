//! Cloud machines handled in batches from the command line. A batch is
//! opened when the window asks the user about it, and kept, bounded, so a
//! caller can follow it with `cloud wait` until every machine in it is
//! settled. Workers update it from their own threads, so it is shared behind
//! a mutex; the window and the control server read it on the UI thread.

pub(crate) mod destroy;

use super::CloudProvider;
use crate::control::batch::{BatchInfo, BatchKind, DeviceInfo, DeviceState, Phase};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

/// Batches kept for `cloud wait` and `cloud batches`; the oldest go first.
const KEPT: usize = 32;
/// The longest failure kept for a device, in characters.
const ERROR_LIMIT: usize = 500;

/// One saved device a batch acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) provider: CloudProvider,
    /// The machine's ID at the provider.
    pub(crate) id: String,
    /// The device's endpoint ID.
    pub(crate) endpoint: String,
    pub(crate) label: String,
}

impl Target {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            provider: self.provider.key().to_owned(),
            id: self.id.clone(),
            endpoint: self.endpoint.clone(),
            label: self.label.clone(),
            state: DeviceState::Waiting,
            error: None,
        }
    }
}

/// Every batch the app keeps. Cloning shares it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Registry(Arc<Mutex<Inner>>);

impl gpui::Global for Registry {}

#[derive(Debug, Default)]
struct Inner {
    next: u64,
    batches: VecDeque<BatchInfo>,
}

impl Inner {
    fn batch(&mut self, id: u64) -> Option<&mut BatchInfo> {
        self.batches.iter_mut().find(|batch| batch.id == id)
    }
}

impl Registry {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A worker that panicked mid-update leaves whole values behind.
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A new batch over `targets`, waiting for the user. The oldest batch
    /// goes once more than [`KEPT`] are kept.
    pub(crate) fn open(&self, kind: BatchKind, targets: &[Target]) -> Pending {
        let mut inner = self.lock();
        inner.next += 1;
        let id = inner.next;
        inner.batches.push_back(BatchInfo {
            id,
            kind,
            phase: Phase::Confirming,
            devices: targets.iter().map(Target::info).collect(),
        });
        while inner.batches.len() > KEPT {
            inner.batches.pop_front();
        }
        Pending {
            registry: self.clone(),
            id,
            targets: targets.to_vec(),
            answered: false,
        }
    }

    pub(crate) fn get(&self, id: u64) -> Option<BatchInfo> {
        self.lock().batch(id).cloned()
    }

    pub(crate) fn all(&self) -> Vec<BatchInfo> {
        self.lock().batches.iter().cloned().collect()
    }

    /// Each kept batch's phase, cheaply, for the event log's every tick.
    pub(crate) fn phases(&self) -> Vec<(u64, Phase)> {
        self.lock()
            .batches
            .iter()
            .map(|batch| (batch.id, batch.phase))
            .collect()
    }

    /// Settle every device of a batch the user turned down.
    fn cancel(&self, id: u64) {
        let mut inner = self.lock();
        let Some(batch) = inner.batch(id) else {
            return;
        };
        if batch.phase != Phase::Confirming {
            return;
        }
        batch.phase = Phase::Cancelled;
        for device in &mut batch.devices {
            device.state = DeviceState::Cancelled;
        }
    }

    /// Start a batch the user accepted; whether it was still waiting.
    fn confirm(&self, id: u64) -> bool {
        let mut inner = self.lock();
        let Some(batch) = inner.batch(id) else {
            return false;
        };
        if batch.phase != Phase::Confirming {
            return false;
        }
        batch.phase = if batch.devices.is_empty() {
            Phase::Done
        } else {
            Phase::Running
        };
        true
    }

    /// Record device `index`'s state. The batch is done once every device
    /// is settled.
    pub(crate) fn update(&self, id: u64, index: usize, state: DeviceState, error: Option<&str>) {
        let mut inner = self.lock();
        let Some(batch) = inner.batch(id) else {
            return;
        };
        let Some(device) = batch.devices.get_mut(index) else {
            return;
        };
        device.state = state;
        device.error = error.map(|error| error.chars().take(ERROR_LIMIT).collect());
        if batch.phase == Phase::Running
            && batch.devices.iter().all(|device| device.state.settled())
        {
            batch.phase = Phase::Done;
        }
    }
}

/// A batch the window is asking the user about. Dropping it unanswered, as
/// closing the dialog does, cancels the batch.
#[derive(Debug)]
pub(crate) struct Pending {
    registry: Registry,
    id: u64,
    targets: Vec<Target>,
    answered: bool,
}

impl Pending {
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn targets(&self) -> &[Target] {
        &self.targets
    }

    /// The user accepted: the batch, its targets, and the registry to
    /// report to, or `None` when the batch was already settled.
    pub(crate) fn confirm(mut self) -> Option<(Registry, u64, Vec<Target>)> {
        self.answered = true;
        let targets = std::mem::take(&mut self.targets);
        self.registry
            .confirm(self.id)
            .then(|| (self.registry.clone(), self.id, targets))
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.answered {
            self.registry.cancel(self.id);
        }
    }
}

#[cfg(test)]
mod tests;
