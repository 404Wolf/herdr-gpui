//! `config`, `theme`, `font`, `keybinds`, `caffeine`, `sound`, `bell`,
//! `update`, and `integrations`.
use super::{Context, Output};
use crate::cli::control::{
    CaffeineCommand, ConfigCommand, IntegrationsCommand, KeybindsCommand, SoundCommand,
    ThemeCommand, UpdateCommand,
};

fn todo() -> crate::Result<Output> {
    Err(crate::Error::ControlRejected {
        code: crate::control::ErrorCode::Unsupported,
        message: "Not implemented yet".into(),
    })
}

pub(super) fn config(_: ConfigCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn theme(_: ThemeCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn font(_: ThemeCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn keybinds(_: KeybindsCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn caffeine(_: CaffeineCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn sound(_: SoundCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn bell(_: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn update(_: UpdateCommand, _: &Context) -> crate::Result<Output> {
    todo()
}

pub(super) fn integrations(_: IntegrationsCommand, _: &Context) -> crate::Result<Output> {
    todo()
}
