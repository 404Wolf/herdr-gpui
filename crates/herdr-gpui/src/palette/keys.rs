//! Keyboard handling while the palette has focus.

use super::{AgentFilter, Filter};
use crate::HerdrWindow;
use gpui::*;

impl HerdrWindow {
    pub(crate) fn palette_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Herdr's navigate-mode keys move a Go To list as its arrows do.
        let step = match event.keystroke.key.as_str() {
            "up" => Some(true),
            "down" => Some(false),
            _ => self
                .menu
                .palette
                .as_ref()
                .filter(|palette| palette.filter == Filter::Navigation)
                .and_then(|_| self.keymap().navigates_workspace(&event.keystroke)),
        };
        let Some(palette) = &mut self.menu.palette else {
            return;
        };
        if palette.search.read(cx).is_composing() {
            return;
        }
        // A Go To navigation binding keeps its key; otherwise Alt and a
        // navigator letter filter by agent status without typing the letter.
        if step.is_none()
            && let Some(agent_filter) = AgentFilter::from_keystroke(&event.keystroke)
        {
            cx.stop_propagation();
            window.prevent_default();
            self.set_palette_agent_filter(agent_filter, cx);
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                cx.stop_propagation();
                window.prevent_default();
                self.dismiss_menu(window, cx);
            }
            _ if !palette.filtered.is_empty() && step.is_some() => {
                let up = step == Some(true);
                cx.stop_propagation();
                window.prevent_default();
                let count = palette.filtered.len();
                palette.selected = (palette.selected + if up { count - 1 } else { 1 }) % count;
                palette
                    .scroll
                    .scroll_to_item(palette.selected, ScrollStrategy::Center);
                cx.notify();
            }
            "tab" => {
                cx.stop_propagation();
                window.prevent_default();
                let index = Filter::ALL
                    .iter()
                    .position(|filter| *filter == palette.filter)
                    .unwrap_or(0);
                let step = if event.keystroke.modifiers.shift {
                    Filter::ALL.len() - 1
                } else {
                    1
                };
                let filter = Filter::ALL[(index + step) % Filter::ALL.len()];
                self.set_palette_filter(filter, cx);
            }
            "enter" => {
                cx.stop_propagation();
                window.prevent_default();
                if let Some(entry) = palette.selected_entry() {
                    let action = entry.action.clone();
                    self.activate_palette(action, window, cx);
                }
            }
            _ => {}
        }
    }
}
