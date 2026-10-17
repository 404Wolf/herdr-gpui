//! `link`: a `herdr-gpui://` link the shell started a second copy of the app
//! with, from a jump list entry, a notification, or another application.
//! It is parsed again here, since any local process can send one, and it
//! only ever shows something: a checkout, a device, or a notification's pane.

use super::{not_found, window_for};
use crate::{
    control::protocol::{Call, ErrorCode, LinkOp, Response},
    link::Link,
};
use gpui::App;

pub(super) fn serve(call: Call<LinkOp>, cx: &mut App) -> Response {
    let link = match Link::parse(&call.op.url) {
        Ok(link) => link,
        Err(error) => {
            return Response::error(ErrorCode::InvalidRequest, format!("Invalid link: {error}"));
        }
    };
    if let Link::Notification { tag } = &link {
        // The notification knows its own window.
        return if crate::window::system_notifications::open(tag, cx) {
            Response::done("Opened the notification's pane")
        } else {
            not_found("That notification is no longer known")
        };
    }
    let handle = match window_for(&call.caller, cx) {
        Ok(handle) => handle,
        Err(response) => return response,
    };
    crate::link::follow::follow(link, handle, cx)
}

#[cfg(test)]
mod tests;
