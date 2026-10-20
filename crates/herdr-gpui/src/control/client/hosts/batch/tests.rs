#![allow(clippy::unwrap_used)]
use super::*;
use crate::control::protocol::cloud::{BatchKind, DeviceInfo};

fn device(label: &str, state: DeviceState, error: Option<&str>) -> DeviceInfo {
    DeviceInfo {
        provider: "coder".into(),
        id: label.to_lowercase(),
        endpoint: format!("coder:{}", label.to_lowercase()),
        label: label.into(),
        state,
        error: error.map(str::to_owned),
    }
}

fn batch(phase: Phase, devices: Vec<DeviceInfo>) -> BatchInfo {
    BatchInfo {
        id: 3,
        kind: BatchKind::Destroy,
        phase,
        devices,
    }
}

#[test]
fn a_batch_reads_one_device_per_line() {
    let done = batch(
        Phase::Done,
        vec![
            device("One", DeviceState::Destroyed, None),
            device("Two", DeviceState::Failed, Some("Coder denied access")),
            device("Three", DeviceState::Forgotten, None),
        ],
    );
    assert_eq!(
        text(&done),
        "Batch 3: done\n\
         One (coder:one): destroyed\n\
         Two (coder:two): failed: Coder denied access\n\
         Three (coder:three): removed from Herdr; its provider cannot delete it from here"
    );
    let asking = batch(
        Phase::Confirming,
        vec![device("One", DeviceState::Waiting, None)],
    );
    assert_eq!(
        asked_text(&asking),
        "Asked in Herdr UI to destroy 1 device as batch 3; the user confirms there.\n\
         Follow it with `herdr-gpui cloud wait 3`."
    );
    assert!(text(&asking).starts_with("Batch 3: waiting for the user to confirm\n"));
}
