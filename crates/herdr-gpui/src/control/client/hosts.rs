//! `pr`, `host`, `cloud`, `ports`, `usage`, and `system`.
use super::{Context, Output};
use crate::cli::control::{CloudCommand, HostCommand, PortsCommand, PrCommand};

fn todo() -> crate::Result<Output> {
    Err(crate::Error::ControlRejected {
        code: crate::control::ErrorCode::Unsupported,
        message: "Not implemented yet".into(),
    })
}

pub(super) fn pr(_: PrCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn host(_: HostCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn cloud(_: CloudCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn ports(_: PortsCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn usage(_: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn system(_: &Context) -> crate::Result<Output> {
    todo()
}
