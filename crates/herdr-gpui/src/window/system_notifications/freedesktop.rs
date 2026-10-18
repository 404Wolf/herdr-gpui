//! Freedesktop notifications over one shared session-bus connection. GPUI's
//! Linux platform opens a connection per notification and keeps it until the
//! notification reports a close, which a daemon that closes it at once (do
//! not disturb, history-only delivery) never lets it see, so each post leaked
//! one bus peer until the bus refused new ones (#387).
//!
//! The click rule is registered before the first `Notify`, so a click on even
//! the first notification is seen. Closes are not tracked: the registry's
//! bounded record of recent posts is the only per-notification state.

#[cfg(test)]
mod tests;

use super::Registry;
use futures_lite::StreamExt;
use gpui::{App, SystemNotification};
use oo7::zbus::{self, MatchRule, Message, MessageStream, message::Type, zvariant::Value};
use std::collections::HashMap;

const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
/// The action a click on the notification body invokes.
const DEFAULT_ACTION: &str = "default";
/// Clicks waiting for the UI thread; older ones are dropped, not queued.
const QUEUED_CLICKS: usize = 16;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("the notification service refused the request")]
    Bus(#[from] zbus::Error),
}

/// How notifications reach the desktop.
#[derive(Default)]
pub(crate) enum Bus {
    /// GPUI posts. Only launches that never connect use it: tests, whose
    /// platform holds no bus.
    #[default]
    Gpui,
    /// Connecting, or the session bus could not be reached: nothing posts.
    Unavailable,
    Connected(zbus::Connection),
}

/// Opens the shared connection and routes clicks for as long as it lives.
pub(super) fn connect(cx: &mut App) {
    cx.global_mut::<Registry>().bus = Bus::Unavailable;
    cx.spawn(async move |cx| {
        let mut clicks = match listen().await {
            Ok((connection, clicks)) => {
                cx.update(|cx| cx.global_mut::<Registry>().bus = Bus::Connected(connection));
                clicks
            }
            Err(error) => {
                tracing::warn!(error = ?error, "System notifications are off: no session bus");
                return;
            }
        };
        while let Some(message) = clicks.next().await {
            let Some(id) = message.ok().as_ref().and_then(clicked) else {
                continue;
            };
            cx.update(|cx| {
                let tag = cx
                    .global::<Registry>()
                    .find_bus_id(id)
                    .map(|p| p.tag.clone());
                if let Some(tag) = tag {
                    super::open(&tag, cx);
                }
            });
        }
    })
    .detach();
}

async fn listen() -> Result<(zbus::Connection, MessageStream), Error> {
    let connection = zbus::Connection::session().await?;
    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .path(PATH)?
        .interface(NAME)?
        .member("ActionInvoked")?
        .build();
    // Returns once the bus has added the rule.
    let clicks = MessageStream::for_match_rule(rule, &connection, Some(QUEUED_CLICKS)).await?;
    Ok((connection, clicks))
}

/// The daemon's id of the notification whose body was clicked.
fn clicked(message: &Message) -> Option<u32> {
    let (id, action) = message.body().deserialize::<(u32, String)>().ok()?;
    (action == DEFAULT_ACTION).then_some(id)
}

/// Posts `notification`, replacing the daemon's `replaces` (0 for none), and
/// returns the daemon's id for it.
pub(super) async fn show(
    connection: &zbus::Connection,
    notification: &SystemNotification,
    replaces: u32,
) -> Result<u32, Error> {
    // Silent: sounds stay with the sound service.
    let hints = HashMap::from([("suppress-sound", Value::from(true))]);
    let reply = connection
        .call_method(
            Some(NAME),
            PATH,
            Some(NAME),
            "Notify",
            &(
                crate::constants::WINDOW_TITLE,
                replaces,
                "",
                notification.title.as_ref(),
                notification.body.as_ref(),
                [DEFAULT_ACTION, DEFAULT_ACTION].as_slice(),
                hints,
                -1_i32,
            ),
        )
        .await?;
    Ok(reply.body().deserialize::<u32>()?)
}
