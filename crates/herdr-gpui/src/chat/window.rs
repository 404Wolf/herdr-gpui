//! The window's side of chat panes: attaching to the panes its daemon
//! reports, moving the keyboard between a chat's composer and the terminal
//! as Herdr's focus moves, and sending what the user asks.

use super::{
    chats::{self, Connect},
    discovery,
};
use crate::{HerdrWindow, search_input::SearchInput};
use gpui::{prelude::*, *};
use herdr_chat_protocol::{Command, MAX_PROMPT_SIZE, Status, assist};
use std::time::Instant;

impl HerdrWindow {
    /// Runs every tick: attaches to the selected daemon's chat panes when it
    /// is local, applies what their programs sent, and keeps each composer
    /// styled and holding the keyboard while its pane is focused.
    pub(crate) fn poll_chats(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.poll_chats_with(&mut chats::connect, window, cx);
    }

    /// [`Self::poll_chats`], connecting through `connect`.
    pub(super) fn poll_chats_with(
        &mut self,
        connect: &mut Connect<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let local = self
            .endpoints
            .get(self.selected_endpoint)
            .is_some_and(|endpoint| discovery::local(&endpoint.connection.target, &self.live));
        let targets = self
            .live
            .snapshot
            .as_deref()
            .map(|snapshot| discovery::targets(snapshot, local))
            .unwrap_or_default();
        self.locate_chat_program(cx);
        self.poll_chat_start(cx);
        let now = Instant::now();
        let mut changed = self.chats.reconcile(targets, now, connect);
        changed |= self.chats.poll(now);
        let mut font = self.config.ui.clone();
        font.size = font.size.max(13.);
        let theme = self.theme.clone();
        for pane in self.chats.panes_mut() {
            if !pane.available() {
                continue;
            }
            let composer = pane.composer.get_or_insert_with(|| {
                changed = true;
                cx.new(|cx| {
                    let mut input = SearchInput::new(cx);
                    input.set_frameless(cx);
                    input
                })
            });
            let accepts = pane.session.transcript().status.accepts_prompts();
            // The selected command shows as ghost text after the typed name.
            let text = composer.read(cx).text().to_owned();
            let found = assist::completions(pane.session.transcript(), &text);
            if pane.completion >= found.len() {
                pane.completion = 0;
            }
            let ghost = found
                .get(pane.completion)
                .and_then(|command| command.name.strip_prefix(text.get(1..).unwrap_or("")))
                .map(str::to_owned);
            composer.update(cx, |input, cx| {
                input.set_completion(ghost, cx);
                input.set_appearance(font.clone(), theme.clone(), cx);
                input.set_placeholder(
                    if accepts {
                        "Message the agent\u{2026}"
                    } else {
                        "The agent cannot take prompts"
                    },
                    cx,
                );
            });
        }
        // A running turn's timer moves once a second.
        let second = super::now_ms() / 1000;
        if self
            .chats
            .panes()
            .any(|pane| pane.session.transcript().turn.is_some())
            && self.chats.ticked != second
        {
            self.chats.ticked = second;
            changed = true;
        }
        self.follow_chat_focus(window, cx);
        // The views are drawn with the terminal, so a surface redraw shows
        // them without rebuilding the cached sidebar.
        if changed {
            self.redraw_terminal(cx);
        }
    }

    /// The composer of the chat over the Herdr-focused pane, when the
    /// window shows that chat now.
    fn focused_chat_composer(&self) -> Option<Entity<SearchInput>> {
        let surface = self.live.surface.as_deref()?;
        if surface.popup.is_some() || !self.live.surface_ready() {
            return None;
        }
        let pane_id = self.live.snapshot.as_ref()?.focused_pane_id.as_deref()?;
        if !self.chats.shown(pane_id) || !surface.panes.iter().any(|p| p.pane_id == pane_id) {
            return None;
        }
        self.chats.pane(pane_id)?.composer.clone()
    }

    /// Typing reaches a chat only through its composer: the program's own
    /// view under it is hidden. So the composer takes the keyboard from the
    /// terminal while its pane is focused, and gives it back once Herdr's
    /// focus moves on or a popup covers the panes.
    fn follow_chat_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.focused_chat_composer();
        let holding = self.chats.panes().find_map(|pane| {
            pane.composer
                .as_ref()
                .filter(|input| input.read(cx).focus.is_focused(window))
                .cloned()
        });
        match (holding, target) {
            (Some(holding), target) if target.as_ref() != Some(&holding) => {
                let focus = target.map_or_else(|| self.focus.clone(), |t| t.read(cx).focus.clone());
                window.focus(&focus, cx);
            }
            (None, Some(target))
                if self.focus.is_focused(window)
                    && self.menu.page.is_none()
                    && !self.copy_mode_active() =>
            {
                let focus = target.read(cx).focus.clone();
                window.focus(&focus, cx);
            }
            _ => {}
        }
    }

    /// Whether a chat's composer holds the keyboard, so the terminal must
    /// not act on a keystroke bubbling out of it.
    pub(crate) fn chat_focused(&self, window: &Window, cx: &App) -> bool {
        self.chats.panes().any(|pane| {
            pane.composer
                .as_ref()
                .is_some_and(|input| input.read(cx).focus.is_focused(window))
        })
    }

    /// Sends `command` to the chat over `pane_id`; a failure shows in the view.
    pub(super) fn chat_send(&mut self, pane_id: &str, command: Command, cx: &mut Context<Self>) {
        let Some(pane) = self.chats.pane_mut(pane_id) else {
            return;
        };
        pane.error = pane.send(command).err().map(|error| error.to_string());
        cx.notify();
    }

    /// Enter sends the draft, Shift-Enter starts a new line of it, and
    /// Escape cancels a running turn.
    pub(super) fn chat_key_down(
        &mut self,
        pane_id: &str,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.chats.pane_mut(pane_id) else {
            return;
        };
        let Some(input) = pane.composer.clone() else {
            return;
        };
        if input.read(cx).is_composing() {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        let shift_only =
            modifiers.shift && !modifiers.control && !modifiers.alt && !modifiers.platform;
        match event.keystroke.key.as_str() {
            "enter" if shift_only => {
                let line = input.read(cx).text().to_owned();
                pane.draft.push(line);
                input.update(cx, |input, cx| input.clear(cx));
            }
            "enter" if !modifiers.modified() => self.submit_chat_prompt(pane_id, &input, cx),
            "escape"
                if !modifiers.modified() && pane.session.transcript().status == Status::Working =>
            {
                self.chat_send(pane_id, Command::Cancel, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }

    /// Keys the composer would otherwise take: Tab and Up/Down choose a
    /// slash command while some match, Up/Down recall earlier prompts from an
    /// empty composer, and Backspace there takes back the draft's last line.
    pub(super) fn chat_capture_key(
        &mut self,
        pane_id: &str,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.modifiers.modified() {
            return;
        }
        let Some(pane) = self.chats.pane_mut(pane_id) else {
            return;
        };
        let Some(input) = pane.composer.clone() else {
            return;
        };
        if input.read(cx).is_composing() {
            return;
        }
        let text = input.read(cx).text().to_owned();
        let found = assist::completions(pane.session.transcript(), &text).len();
        let key = event.keystroke.key.as_str();
        match key {
            "tab" if found > 0 => {
                let Some(name) = assist::completions(pane.session.transcript(), &text)
                    .get(pane.completion)
                    .map(|command| command.name.clone())
                else {
                    return;
                };
                self.complete_chat_command(pane_id, &name, window, cx);
            }
            "up" | "down" if found > 0 => {
                pane.completion = if key == "up" {
                    pane.completion.saturating_sub(1)
                } else {
                    (pane.completion + 1).min(found - 1)
                };
            }
            "up" | "down" if text.is_empty() || pane.history.recalling() => {
                let recalled = if key == "up" {
                    pane.history.older()
                } else {
                    pane.history.newer()
                };
                let Some(recalled) = recalled.map(str::to_owned) else {
                    return;
                };
                input.update(cx, |input, cx| {
                    input.clear(cx);
                    input.replace_text_in_range(None, &recalled, window, cx);
                });
            }
            "backspace" if text.is_empty() => {
                let Some(line) = pane.draft.pop() else {
                    return;
                };
                input.update(cx, |input, cx| {
                    input.replace_text_in_range(None, &line, window, cx);
                });
            }
            _ => {
                if key != "up" && key != "down" {
                    pane.history.stop();
                }
                return;
            }
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }

    /// Replaces the composer's text with `/name `, ready for its input.
    pub(super) fn complete_chat_command(
        &mut self,
        pane_id: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.chats.pane_mut(pane_id) else {
            return;
        };
        pane.completion = 0;
        let Some(input) = pane.composer.clone() else {
            return;
        };
        let text = format!("/{name} ");
        input.update(cx, |input, cx| {
            input.clear(cx);
            input.replace_text_in_range(None, &text, window, cx);
        });
        let focus = input.read(cx).focus.clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    fn submit_chat_prompt(
        &mut self,
        pane_id: &str,
        input: &Entity<SearchInput>,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.chats.pane_mut(pane_id) else {
            return;
        };
        let text = prompt_text(&pane.draft, input.read(cx).text());
        if text.trim().is_empty() {
            return;
        }
        if text.len() > MAX_PROMPT_SIZE {
            pane.error = Some("The prompt is longer than the agent accepts".into());
            return;
        }
        pane.history.push(&text);
        match pane.send(Command::Prompt { text }) {
            Ok(()) => {
                pane.error = None;
                pane.draft.clear();
                input.update(cx, |input, cx| input.clear(cx));
            }
            Err(error) => pane.error = Some(error.to_string()),
        }
    }

    /// Shows the terminal under the chat over `pane_id`, or the chat again;
    /// `false` when the pane has no chat.
    pub(crate) fn set_chat_raw(
        &mut self,
        pane_id: &str,
        raw: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.chats.set_raw(pane_id, raw) {
            return false;
        }
        if raw && self.chat_focused(window, cx) {
            window.focus(&self.focus, cx);
        }
        self.follow_chat_focus(window, cx);
        cx.notify();
        true
    }
}

/// The prompt a draft and the composer's line make.
pub(crate) fn prompt_text(draft: &[String], line: &str) -> String {
    if draft.is_empty() {
        return line.to_owned();
    }
    let mut text = draft.join("\n");
    text.push('\n');
    text.push_str(line);
    text
}
