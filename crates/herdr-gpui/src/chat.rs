//! A native view of an existing Claude pane. Herdr retains terminal ownership;
//! this client only reads the agent's transcript and explicitly submits prompts.
mod error;
#[cfg(any(test, feature = "mockup"))]
mod fixture;
mod input;
mod input_guard;
mod rows;
mod session;
mod view;
mod worker;

use crate::{HerdrWindow, browser::Shown};
pub use error::{Error, Result};
use gpui::{prelude::*, *};
use herdr_client::{chat::Mode, protocol::AgentStatus};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    endpoint: String,
    boot: String,
    tab: String,
    pane: String,
}
struct Entry {
    key: Key,
    view: Entity<view::View>,
    _observe: Subscription,
}

#[derive(Default)]
pub(crate) struct Chats {
    entries: Vec<Entry>,
    current: Option<Key>,
}

#[cfg(feature = "mockup")]
// External native QA files call this; the default mockup gallery need not.
#[allow(dead_code)]
pub(crate) fn preview(
    config: crate::config::Config,
    theme: crate::config::Theme,
    cx: &mut App,
) -> AnyView {
    cx.new(|cx| view::View::fixture(config, theme, cx)).into()
}

impl HerdrWindow {
    fn chat_key(&self, cx: &App) -> Option<Key> {
        let endpoint = self.endpoints.get(self.selected_endpoint)?;
        if !worker::supports(&endpoint.connection.target) {
            return None;
        }
        let snapshot = self.live.snapshot.as_ref()?;
        let tab = snapshot.focused_tab_id.as_ref()?;
        // While reading chat, daemon focus changes must not redirect its draft.
        if let Some(entry) = self.chats.entries.iter().find(|entry| {
            entry.key.endpoint == endpoint.id
                && entry.key.boot == snapshot.boot_id
                && &entry.key.tab == tab
                && entry.view.read(cx).mode == Mode::Chat
        }) {
            return Some(entry.key.clone());
        }
        let pane = snapshot.focused_pane_id.as_ref()?;
        snapshot
            .agents
            .iter()
            .find(|agent| &agent.pane_id == pane && agent.agent.as_deref() == Some("claude"))?;
        Some(Key {
            endpoint: endpoint.id.clone(),
            boot: snapshot.boot_id.clone(),
            tab: tab.clone(),
            pane: pane.clone(),
        })
    }

    pub(crate) fn poll_chat(&mut self, cx: &mut Context<Self>) {
        let key = self.chat_key(cx);
        if self.chats.current != key {
            self.chats.current = key.clone();
            cx.notify();
        }
        if let Some(key) = &key
            && !self.chats.entries.iter().any(|e| &e.key == key)
        {
            let previous = self
                .chats
                .entries
                .iter()
                .position(|entry| {
                    entry.key.endpoint == key.endpoint
                        && entry.key.pane == key.pane
                        && entry.key.boot != key.boot
                })
                .map(|index| {
                    let entry = self.chats.entries.remove(index);
                    let saved = entry.view.read(cx).saved_draft(cx);
                    entry.view.update(cx, |view, cx| view.suspend(cx));
                    saved
                });
            if self.chats.entries.len() >= 8
                && let Some(index) = self
                    .chats
                    .entries
                    .iter()
                    .position(|e| e.view.read(cx).can_forget(cx))
            {
                self.chats.entries.remove(index);
            }
            // Do not silently evict drafts. Further panes remain terminal-only.
            if self.chats.entries.len() < 8 {
                let label = self
                    .live
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.tabs.iter().find(|t| t.tab_id == key.tab))
                    .map(|t| t.label.clone())
                    .unwrap_or_else(|| key.tab.clone());
                let view = cx.new(|cx| {
                    let mut view = view::View::new(
                        key.pane.clone(),
                        label,
                        self.config.clone(),
                        self.theme.clone(),
                        self.focus.clone(),
                        cx,
                    );
                    if let Some((mode, draft)) = previous
                        && (mode == Mode::Chat || !draft.is_empty())
                    {
                        view.restore_detached_draft(mode, &draft, cx);
                    }
                    view
                });
                let observe = cx.observe(&view, |_, _, cx| cx.notify());
                self.chats.entries.push(Entry {
                    key: key.clone(),
                    view,
                    _observe: observe,
                });
                cx.notify();
            }
        }
        let showing = self
            .primary_group()
            .or_else(|| self.active_group())
            .is_some_and(|group| self.group_shown(group, cx) == Shown::Terminal)
            && self.menu.page.is_none();
        let endpoint = self.endpoints.get(self.selected_endpoint);
        let target = endpoint.map(|e| e.connection.target.clone());
        let handle = endpoint.and_then(|e| e.connection.handle.clone());
        let connected = self.live.status.is_connected();
        for entry in &self.chats.entries {
            if Some(&entry.key) == key.as_ref() && showing {
                let status = self
                    .live
                    .snapshot
                    .as_ref()
                    .and_then(|s| {
                        s.agents.iter().find(|a| {
                            a.pane_id == entry.key.pane && a.agent.as_deref() == Some("claude")
                        })
                    })
                    .map(|a| a.agent_status)
                    .unwrap_or(AgentStatus::Unknown);
                if let Some(target) = &target {
                    entry.view.update(cx, |view, cx| {
                        view.appearance(&self.config, &self.theme, cx);
                        view.tick(target, handle.clone(), connected, status, cx)
                    });
                }
            } else {
                entry.view.update(cx, |view, cx| view.suspend(cx));
            }
        }
    }

    pub(crate) fn render_agent_chat(&self, terminal: AnyElement, cx: &App) -> AnyElement {
        let Some(entry) = self
            .chats
            .entries
            .iter()
            .find(|e| Some(&e.key) == self.chats.current.as_ref())
        else {
            return terminal;
        };
        let chat = entry.view.read(cx).mode == Mode::Chat;
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(entry.view.clone())
            .when(!chat, |el| el.child(terminal))
            .into_any_element()
    }

    /// The control socket changes only this window's current, verified target.
    pub(crate) fn chat_mode(
        &mut self,
        asked: Option<Mode>,
        caller_pane: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(Mode, String)> {
        self.poll_chat(cx);
        let entry = self
            .chats
            .entries
            .iter()
            .find(|e| Some(&e.key) == self.chats.current.as_ref())
            .ok_or(Error::Unsupported)?;
        if caller_pane.is_some_and(|pane| pane != entry.key.pane) {
            return Err(Error::TargetChanged);
        }
        if let Some(mode) = asked {
            entry
                .view
                .update(cx, |view, cx| view.set_mode(mode, window, cx));
        }
        Ok((entry.view.read(cx).mode, entry.key.pane.clone()))
    }
}

#[cfg(test)]
mod tests;
