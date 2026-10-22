//! Turns chat changes into push notifications for every device that asked
//! for them. It keeps its own copy of each transcript, stepped with the
//! shared reducer, so a trigger sees the state before each change. Sending
//! is blocking HTTPS, so it runs on the blocking pool; a push service that
//! says the subscription is gone drops it from the device.

use crate::{
    control::DeviceId,
    hub::Event,
    push::{Chat, Delivery, Notification, Outcome, Subscription, Throttle, decide},
    web::{State, now_ms},
};
use herdr_chat_protocol::Transcript;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::broadcast::error::RecvError;

/// How often held-back notifications are checked.
const TICK: Duration = Duration::from_secs(1);

pub(super) async fn run(state: Arc<State>) {
    if state.push.is_none() && state.apns.is_none() {
        return;
    }
    let mut live = super::live::LiveActivities::default();
    let mut events = state.hub.subscribe_all();
    let mut transcripts: HashMap<String, Transcript> = HashMap::new();
    let mut throttles: HashMap<DeviceId, Throttle> = HashMap::new();
    let mut ticker = tokio::time::interval(TICK);
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok((pane, Event::Reset)) => {
                    if let Some(link) = state.hub.chat(&pane) {
                        transcripts.insert(pane, link.view().transcript);
                    }
                }
                Ok((pane, Event::Change { change, .. })) => {
                    let Some(before) = transcripts.get_mut(&pane) else {
                        if let Some(link) = state.hub.chat(&pane) {
                            transcripts.insert(pane, link.view().transcript);
                        }
                        continue;
                    };
                    let notification = notification(&state, &pane, before, &change);
                    live.change(&state, &pane, before, &change);
                    if before.apply(&change).is_err()
                        && let Some(link) = state.hub.chat(&pane)
                    {
                        *before = link.view().transcript;
                    }
                    if let Some(notification) = notification {
                        offer(&state, &mut throttles, notification);
                    }
                }
                Ok((_, Event::Connection)) => {}
                Err(RecvError::Lagged(_)) => transcripts.clear(),
                Err(RecvError::Closed) => return,
            },
            _ = ticker.tick() => {
                live.tick(&state);
                let now = now_ms();
                let due: Vec<(DeviceId, Notification)> = throttles
                    .iter_mut()
                    .flat_map(|(device, throttle)| {
                        throttle.due(now).into_iter().map(|n| (device.clone(), n)).collect::<Vec<_>>()
                    })
                    .collect();
                for (device, notification) in due {
                    send(&state, device, notification);
                }
                // Forget chats that left the layout.
                let layout = state.hub.layout();
                transcripts.retain(|pane, _| layout.chat(pane).is_some());
            }
        }
    }
}

fn notification(
    state: &State,
    pane: &str,
    before: &Transcript,
    change: &herdr_chat_protocol::Change,
) -> Option<Notification> {
    let layout = state.hub.layout();
    let (workspace, row) = layout.chat(pane)?;
    let agent = if before.agent.is_empty() {
        row.agent.as_deref().unwrap_or("The agent")
    } else {
        &before.agent
    };
    decide(
        &Chat {
            agent,
            pane,
            title: &row.title,
            workspace: &workspace.label,
        },
        before,
        change,
        state.privacy,
    )
}

/// Offers `notification` to every subscribed device's throttle and sends
/// what passes now.
fn offer(
    state: &Arc<State>,
    throttles: &mut HashMap<DeviceId, Throttle>,
    notification: Notification,
) {
    let subscribed: Vec<DeviceId> = state
        .auth()
        .devices()
        .list(|_| false)
        .into_iter()
        .filter(|device| device.push)
        .map(|device| device.id)
        .collect();
    throttles.retain(|device, _| subscribed.contains(device));
    let now = now_ms();
    for device in subscribed {
        if let Some(passed) = throttles
            .entry(device.clone())
            .or_default()
            .offer(notification.clone(), now)
        {
            send(state, device, passed);
        }
    }
}

/// Questions waiting across every chat, for the app badge.
pub(super) fn waiting(state: &State) -> u32 {
    let layout = state.hub.layout();
    let total: usize = layout
        .chats()
        .filter_map(|chat| state.hub.chat(&chat.pane))
        .map(|link| link.summary().questions)
        .sum();
    u32::try_from(total).unwrap_or(u32::MAX)
}

fn send(state: &Arc<State>, device: DeviceId, notification: Notification) {
    let subscription = state
        .auth()
        .devices()
        .get(&device)
        .and_then(|record| record.push.clone())
        .and_then(|value| serde_json::from_value::<Subscription>(value).ok());
    let Some(subscription) = subscription else {
        return;
    };
    let message = notification.message(waiting(state));
    let delivery = Delivery::for_notification(&notification);
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let Some(sender) = &state.push else {
            return;
        };
        if let Ok(Outcome::Gone) = sender.send(&subscription, &message, &delivery) {
            let _ = state.auth().devices_mut().set_push(&device, None);
        }
    });
}
