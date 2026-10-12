//! `notes`, `checkpoint`, `scripts`, `teleport`, and `fan-out`.
use super::{Context, Output};
use crate::cli::control::{CheckpointCommand, FanOut, NotesCommand, Script};

fn todo() -> crate::Result<Output> {
    Err(crate::Error::ControlRejected {
        code: crate::control::ErrorCode::Unsupported,
        message: "Not implemented yet".into(),
    })
}

pub(super) fn notes(_: NotesCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn checkpoint(_: CheckpointCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn scripts(_: Option<Script>, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn teleport(_: Option<&str>, _: String, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn fan_out(_: FanOut, _: &Context) -> crate::Result<Output> {
    todo()
}
