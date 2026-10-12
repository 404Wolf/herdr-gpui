//! Placeholder until this family is implemented.
use super::{Watches, socket};
use crate::control::protocol::{Call, ErrorCode, Response, settings::SettingsOp};
use gpui::App;

pub(super) fn serve(_: Call<SettingsOp>, incoming: socket::Incoming, _: &mut Watches, _: &mut App) {
    incoming.respond(Response::error(
        ErrorCode::Unsupported,
        "Not implemented yet",
    ));
}
