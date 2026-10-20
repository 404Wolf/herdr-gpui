#![allow(clippy::unwrap_used)]
use super::*;
use crate::control::protocol::{Call, Caller, MAX_WAIT_SECONDS, Request, Response};

fn device(state: DeviceState) -> DeviceInfo {
    DeviceInfo {
        provider: "coder".into(),
        id: "w1".into(),
        endpoint: "coder:w1".into(),
        label: "Build box".into(),
        state,
        error: None,
    }
}

#[test]
fn cloud_requests_have_a_stable_wire_form() {
    let request = Request::Cloud(Call {
        caller: Caller::default(),
        op: CloudOp::Destroy {
            devices: vec!["coder:w1".into()],
            batch: None,
        },
    });
    let text = serde_json::to_string(&request).unwrap();
    assert_eq!(
        text,
        r#"{"method":"cloud","caller":{"pane_id":null,"workspace_id":null,"daemon_socket":null},"op":{"op":"destroy","devices":["coder:w1"]}}"#
    );
    assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), request);
    for op in [
        CloudOp::Destroy {
            devices: Vec::new(),
            batch: Some(3),
        },
        CloudOp::Wait {
            batch: 3,
            wait_seconds: 30,
        },
        CloudOp::Batches,
    ] {
        let request = Request::Cloud(Call {
            caller: Caller::default(),
            op,
        });
        let text = serde_json::to_string(&request).unwrap();
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), request);
    }
}

#[test]
fn unknown_cloud_operations_and_fields_are_rejected() {
    for invalid in [
        r#"{"method":"cloud","caller":{},"op":{"op":"create"}}"#,
        r#"{"method":"cloud","caller":{},"op":{"op":"wait"}}"#,
        r#"{"method":"cloud","caller":{},"op":{"op":"wait","batch":-1,"wait_seconds":1}}"#,
        r#"{"method":"cloud","caller":{},"op":{"op":"destroy","force":true}}"#,
    ] {
        assert!(
            serde_json::from_str::<Request>(invalid).is_err(),
            "{invalid}"
        );
    }
}

#[test]
fn a_wait_is_bounded_like_every_other() {
    let request = Request::Cloud(Call {
        caller: Caller::default(),
        op: CloudOp::Wait {
            batch: 1,
            wait_seconds: u64::MAX,
        },
    });
    assert_eq!(request.wait_seconds(), MAX_WAIT_SECONDS);
}

#[test]
fn replies_carry_each_device_and_round_trip() {
    let mut failed = device(DeviceState::Failed);
    failed.error = Some("Coder denied access".into());
    let response = Response::Cloud {
        reply: Reply::Batch {
            batch: BatchInfo {
                id: 3,
                kind: BatchKind::Destroy,
                phase: Phase::Done,
                devices: vec![device(DeviceState::Destroyed), failed],
            },
        },
    };
    let text = serde_json::to_string(&response).unwrap();
    assert!(text.contains(r#""status":"cloud""#), "{text}");
    assert!(text.contains(r#""state":"destroyed""#), "{text}");
    assert!(text.contains(r#""error":"Coder denied access""#), "{text}");
    assert_eq!(serde_json::from_str::<Response>(&text).unwrap(), response);
}

#[test]
fn a_batch_succeeds_only_once_done_without_a_failure() {
    let batch = |phase, states: &[DeviceState]| BatchInfo {
        id: 1,
        kind: BatchKind::Destroy,
        phase,
        devices: states.iter().copied().map(device).collect(),
    };
    assert!(
        batch(
            Phase::Done,
            &[DeviceState::Destroyed, DeviceState::Forgotten]
        )
        .succeeded()
    );
    assert!(!batch(Phase::Done, &[DeviceState::Destroyed, DeviceState::Failed]).succeeded());
    assert!(!batch(Phase::Running, &[DeviceState::Destroying]).succeeded());
    assert!(!batch(Phase::Cancelled, &[DeviceState::Cancelled]).succeeded());
    assert!(Phase::Cancelled.settled() && !Phase::Confirming.settled());
    #[cfg(feature = "cloud")]
    assert!(DeviceState::Forgotten.settled() && !DeviceState::Waiting.settled());
}

#[test]
fn phases_are_named_as_the_wire_names_them() {
    for phase in [
        Phase::Confirming,
        Phase::Running,
        Phase::Done,
        Phase::Cancelled,
    ] {
        assert_eq!(serde_json::to_value(phase).unwrap(), phase.name());
    }
}
