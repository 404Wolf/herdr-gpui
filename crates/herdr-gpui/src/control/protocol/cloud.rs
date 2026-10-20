//! Saved cloud machines handled in batches, so an agent can clean up the
//! machines it worked on: destroying several after one confirmation in the
//! window, and following what each batch did to every machine.
use serde::{Deserialize, Serialize};

/// The most devices one batch holds, as many as the device list keeps.
pub(crate) const MAX_DEVICES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CloudOp {
    /// Ask in the window to destroy saved cloud devices: the ones `devices`
    /// names (endpoint ID, machine ID, or label), and the ones batch `batch`
    /// holds. Nothing is deleted until the user confirms there.
    Destroy {
        #[serde(default)]
        devices: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        batch: Option<u64>,
    },
    /// Batch `batch`, once every device in it is settled or `wait_seconds`
    /// pass.
    Wait { batch: u64, wait_seconds: u64 },
    /// The batches the app keeps, oldest first.
    Batches,
}

impl CloudOp {
    pub(crate) fn wait_seconds(&self) -> u64 {
        match self {
            Self::Wait { wait_seconds, .. } => *wait_seconds,
            Self::Destroy { .. } | Self::Batches => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BatchKind {
    Destroy,
}

/// Where a batch is: waiting for the user, working, or finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    /// The window asks the user; nothing has started.
    Confirming,
    Running,
    /// Every device is settled, each one well or not.
    Done,
    /// The user said no, or the dialog closed without an answer.
    Cancelled,
}

impl Phase {
    pub(crate) fn settled(self) -> bool {
        matches!(self, Self::Done | Self::Cancelled)
    }

    /// As the wire and events name it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Confirming => "confirming",
            Self::Running => "running",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DeviceState {
    /// Not started: the batch waits for the user.
    Waiting,
    Destroying,
    /// Deleted at its provider, and forgotten here.
    Destroyed,
    /// Forgotten here only, as its provider cannot delete it from this app.
    Forgotten,
    Failed,
    Cancelled,
}

impl DeviceState {
    #[cfg(feature = "cloud")]
    pub(crate) fn settled(self) -> bool {
        !matches!(self, Self::Waiting | Self::Destroying)
    }

    /// Settled without what was asked happening.
    pub(crate) fn unsuccessful(self) -> bool {
        matches!(self, Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DeviceInfo {
    /// The provider's key, such as `coder`.
    pub provider: String,
    /// The machine's ID at the provider.
    pub id: String,
    /// The device's endpoint ID, `provider:id`.
    pub endpoint: String,
    pub label: String,
    pub state: DeviceState,
    /// Why it failed, for a person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BatchInfo {
    pub id: u64,
    pub kind: BatchKind,
    pub phase: Phase,
    pub devices: Vec<DeviceInfo>,
}

impl BatchInfo {
    /// Whether the batch finished with every device as asked.
    pub(crate) fn succeeded(&self) -> bool {
        self.phase == Phase::Done
            && !self
                .devices
                .iter()
                .any(|device| device.state.unsuccessful())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Reply {
    Batch { batch: BatchInfo },
    Batches { batches: Vec<BatchInfo> },
}

#[cfg(test)]
mod tests;
