//! Placeholder until this family is implemented.
use super::{Watches, socket};
use crate::control::protocol::{Call, ErrorCode, Response, tabs::TabOp};
use gpui::App;

pub(super) fn serve(_: Call<TabOp>, incoming: socket::Incoming, _: &mut Watches, _: &mut App) {
    incoming.respond(Response::error(
        ErrorCode::Unsupported,
        "Not implemented yet",
    ));
}
