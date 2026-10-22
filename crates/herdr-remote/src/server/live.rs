//! Live Activities for the iOS companion: each chat change the APNs rules
//! call for is paced per chat, then sent to every device that registered
//! tokens: the push-to-start token to start an activity, the activity's own
//! token to update or end it. A token APNs calls invalid is forgotten.

use super::notifier::waiting;
use crate::{
    apns::{Event, Live, Outcome, Pacer, Registration, decide},
    control::DeviceId,
    push::Chat,
    web::{State, now_ms},
};
use herdr_chat_protocol::{Change, Status, Transcript};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct LiveActivities {
    pacer: Pacer,
}

impl LiveActivities {
    /// Considers `change` to the chat in `pane`, whose transcript was
    /// `before`.
    pub(super) fn change(
        &mut self,
        state: &Arc<State>,
        pane: &str,
        before: &Transcript,
        change: &Change,
    ) {
        if state.apns.is_none() {
            return;
        }
        let layout = state.hub.layout();
        let Some((workspace, row)) = layout.chat(pane) else {
            return;
        };
        let agent = if before.agent.is_empty() {
            row.agent.as_deref().unwrap_or("The agent")
        } else {
            &before.agent
        };
        let chat = Chat {
            agent,
            pane,
            title: &row.title,
            workspace: &workspace.label,
        };
        let now = now_ms();
        let Some(live) = decide(
            &chat,
            &state.computer,
            &state.fingerprint,
            before,
            change,
            now,
        ) else {
            return;
        };
        if let Some(live) = self.pacer.offer(live, now) {
            dispatch(state, live);
        }
    }

    /// Sends the updates the pacer held back whose time came.
    pub(super) fn tick(&mut self, state: &Arc<State>) {
        if state.apns.is_none() {
            return;
        }
        for live in self.pacer.due(now_ms()) {
            dispatch(state, live);
        }
    }
}

/// Chats with a turn running now, for the Dynamic Island's count.
fn working(state: &State) -> u32 {
    let layout = state.hub.layout();
    let count = layout
        .chats()
        .filter_map(|chat| state.hub.chat(&chat.pane))
        .filter(|link| link.summary().status == Status::Working)
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn dispatch(state: &Arc<State>, mut live: Live) {
    live.state.waiting = waiting(state);
    live.state.working = working(state);
    let registrations: Vec<(DeviceId, Registration)> = state
        .auth()
        .devices()
        .list(|_| false)
        .into_iter()
        .filter_map(|device| {
            let registration = Registration::from_stored(
                state
                    .auth()
                    .devices()
                    .get(&device.id)
                    .and_then(|record| record.apns.as_ref()),
            );
            (!registration.is_empty()).then_some((device.id, registration))
        })
        .collect();
    for (device, registration) in registrations {
        let token = match live.event {
            Event::Start { .. } => registration.push_to_start.clone(),
            _ => registration.activity(&live.pane).cloned(),
        };
        let Some(token) = token else {
            continue;
        };
        let (state, live) = (state.clone(), live.clone());
        tokio::spawn(async move {
            let Some(sender) = &state.apns else {
                return;
            };
            let outcome = sender.send(&token, &live, now_ms() / 1000).await;
            let ended = matches!(live.event, Event::End { .. });
            if !matches!(outcome, Ok(Outcome::Gone)) && !ended {
                return;
            }
            let gone = matches!(outcome, Ok(Outcome::Gone));
            let pane = live.pane.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let mut auth = state.auth();
                let devices = auth.devices_mut();
                let mut registration = Registration::from_stored(
                    devices.get(&device).and_then(|record| record.apns.as_ref()),
                );
                let changed = if gone {
                    registration.forget(&token)
                } else {
                    registration.end(&pane)
                };
                if changed && let Ok(value) = registration.to_stored() {
                    let _ = devices.set_apns(&device, value);
                }
            })
            .await;
        });
    }
}
