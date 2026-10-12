//! `tab`, `code`, `devices`, `orchestrator`, and `review`.
use super::{Context, Output};
use crate::cli::control::{CodeCommand, OrchestratorCommand, TabCommand};

fn todo() -> crate::Result<Output> {
    Err(crate::Error::ControlRejected {
        code: crate::control::ErrorCode::Unsupported,
        message: "Not implemented yet".into(),
    })
}

pub(super) fn list(_pages: bool, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn tab(_: TabCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn code(_: CodeCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn devices(_: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn orchestrator(_: OrchestratorCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn review(_: &Context) -> crate::Result<Output> {
    todo()
}
