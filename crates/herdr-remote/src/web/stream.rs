//! The patch stream: server-sent events that replace regions of the view
//! the phone shows as the chats change. The first event brings the view up
//! to date, so a fragment fetched just before the stream opened cannot miss
//! a change. Bursts coalesce, a lagging stream reloads its view, and the
//! stream counts as the device being connected.

use super::{Device, Shared, fragments::lists, render};
use crate::{control::DeviceId, hub::Event};
use axum::{
    extract::{Query, State},
    response::sse::{Event as Sse, KeepAlive, Sse as Stream},
};
use futures_lite::stream;
use serde::Deserialize;
use serde_json::json;
use std::{convert::Infallible, time::Duration};
use tokio::sync::{broadcast::error::RecvError, mpsc};

/// How long a chat's changes gather before one patch goes out.
const CHAT_COALESCE: Duration = Duration::from_millis(120);
/// How long list changes gather before one patch goes out.
const LIST_COALESCE: Duration = Duration::from_millis(400);
/// Patches queued for a slow phone before the stream gives up.
const QUEUE: usize = 32;

#[derive(Deserialize)]
pub(super) struct Target {
    view: String,
    #[serde(default)]
    pane: Option<String>,
}

/// One open stream, counted against its device until it is dropped.
struct Connected {
    state: Shared,
    device: DeviceId,
}

impl Connected {
    fn new(state: Shared, device: DeviceId) -> Self {
        *state.connected().entry(device.clone()).or_default() += 1;
        Self { state, device }
    }
}

impl Drop for Connected {
    fn drop(&mut self) {
        let mut connected = self.state.connected();
        if let Some(count) = connected.get_mut(&self.device) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                connected.remove(&self.device);
            }
        }
    }
}

pub(super) async fn stream(
    State(state): State<Shared>,
    Device(device): Device,
    Query(target): Query<Target>,
) -> Stream<impl futures_lite::Stream<Item = Result<Sse, Infallible>>> {
    let (sender, receiver) = mpsc::channel(QUEUE);
    let guard = Connected::new(state.clone(), device);
    tokio::spawn(async move {
        let _guard = guard;
        match (target.view.as_str(), target.pane) {
            ("chat", Some(pane)) => follow_chat(&state, &pane, &sender).await,
            ("board", _) => follow_lists(&state, true, &sender).await,
            _ => follow_lists(&state, false, &sender).await,
        }
    });
    let events = stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (Ok(event), receiver))
    });
    Stream::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(20)))
}

fn reload() -> Sse {
    Sse::default().event("reload").data("{}")
}

fn patch(regions: Vec<(&str, String)>, badge: Option<usize>) -> Sse {
    let regions: serde_json::Map<String, serde_json::Value> = regions
        .into_iter()
        .map(|(id, html)| (id.to_owned(), json!(html)))
        .collect();
    let mut body = json!({ "regions": regions });
    if let Some(badge) = badge {
        body["badge"] = json!(badge);
    }
    Sse::default().event("patch").data(body.to_string())
}

/// Sends the chat's regions now and after every burst of changes, until
/// the phone goes away or the chat does.
async fn follow_chat(state: &Shared, pane: &str, sender: &mpsc::Sender<Sse>) {
    let Some(link) = state.hub.chat(pane) else {
        let _ = sender.send(reload()).await;
        return;
    };
    let (_, mut events) = link.subscribe();
    let mut layout = state.hub.watch_layout();
    loop {
        let current = state.hub.layout();
        let Some((workspace, row)) = current.chat(pane) else {
            let _ = sender.send(reload()).await;
            return;
        };
        let regions = render::chat_patches(workspace, row, &link.view(), render::now_ms())
            .into_iter()
            .map(|(id, markup)| (id, markup.into_string()))
            .collect();
        if sender.send(patch(regions, None)).await.is_err() {
            return;
        }
        tokio::select! {
            event = events.recv() => match event {
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => {
                    let _ = sender.send(reload()).await;
                    return;
                }
            },
            changed = layout.changed() => if changed.is_err() { return },
            () = sender.closed() => return,
        }
        tokio::time::sleep(CHAT_COALESCE).await;
        // Whatever else arrived meanwhile is in the next render.
        while events.try_recv().is_ok() {}
    }
}

/// Sends home or the board now and after every burst of changes in any
/// chat or in the layout.
async fn follow_lists(state: &Shared, board: bool, sender: &mpsc::Sender<Sse>) {
    let mut all = state.hub.subscribe_all();
    let mut layout = state.hub.watch_layout();
    loop {
        let (markup, waiting) = lists(state, board);
        if sender
            .send(patch(vec![("lists", markup.into_string())], Some(waiting)))
            .await
            .is_err()
        {
            return;
        }
        loop {
            tokio::select! {
                event = all.recv() => match event {
                    // Streamed text alone does not change a list.
                    Ok((_, Event::Change { change, .. })) if matches!(*change, herdr_chat_protocol::Change::Append { .. }) => continue,
                    Ok(_) | Err(RecvError::Lagged(_)) => break,
                    Err(RecvError::Closed) => return,
                },
                changed = layout.changed() => if changed.is_err() { return } else { break },
                () = sender.closed() => return,
            }
        }
        tokio::time::sleep(LIST_COALESCE).await;
        while all.try_recv().is_ok() {}
    }
}
