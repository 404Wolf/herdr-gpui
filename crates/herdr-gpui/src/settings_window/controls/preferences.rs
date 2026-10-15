use super::*;
use crate::config::{
    Axis, ClipboardToastPosition as ClipboardPosition, MAX_PADDING, Side, preferences::Preference,
};
use herdr_client::protocol::ToastHerdrPosition as Position;

#[cfg(test)]
#[derive(Clone)]
pub(super) struct PreferenceIo {
    pub(super) write: std::sync::Arc<dyn Fn(Preference) -> crate::Result<()> + Send + Sync>,
    pub(super) load: fn() -> crate::Result<super::super::Loaded>,
}

impl SettingsWindow {
    pub(super) fn save_preference(&mut self, edit: Preference, cx: &mut Context<Self>) {
        #[cfg(test)]
        if let Some(io) = self.controls.preference_io.clone() {
            self.save_with(move || (io.write)(edit), io.load, false, cx);
            return;
        }
        self.save_native(move || Config::save_preference(edit), cx);
    }
    pub(in crate::settings_window) fn render_integration_controls(
        &self,
        cx: &mut Context<Self>,
    ) -> Div {
        let query = self.controls.integration_search.read(cx).text().to_owned();
        div().flex().flex_col().gap(px(16.))
            .child(self.controls.integration_search.clone())
            .child(self.source.update(cx, |source, cx| source.render_filtered_integrations(&query, cx))
                .unwrap_or_else(|_| div().child("Open a session window to manage agent integrations. Local preferences remain available.")))
    }

    pub(in crate::settings_window) fn control_switch(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        checked: bool,
        enabled: bool,
    ) -> Stateful<Div> {
        let id = id.into();
        div()
            .id(ElementId::Name(id.clone()))
            .debug_selector(move || id.to_string())
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .py(px(8.))
            .when(enabled, |row| row.cursor_pointer())
            .when(!enabled, |row| row.opacity(0.5))
            .child(label.into())
            .child(crate::toggles::switch(&self.theme, 22., checked))
    }

    pub(in crate::settings_window) fn preference_switch(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        edit: Preference,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.control_switch(id, label, checked, true)
            .on_click(cx.listener(move |this, _, _, cx| this.save_preference(edit, cx)))
    }

    pub(super) fn mouse_switches(&self, cx: &mut Context<Self>) -> [Stateful<Div>; 2] {
        [
            self.preference_switch(
                "settings-smooth-scroll",
                "Smooth scrolling",
                self.config.smooth_scroll,
                Preference::SmoothScroll(!self.config.smooth_scroll),
                cx,
            ),
            self.preference_switch(
                "settings-focus-follows-mouse",
                "Focus follows mouse",
                self.config.focus_follows_mouse,
                Preference::FocusFollowsMouse(!self.config.focus_follows_mouse),
                cx,
            ),
        ]
    }

    /// One switch per close kind, each saving its own `confirm_close_*` key.
    pub(super) fn confirm_close_switches(&self, cx: &mut Context<Self>) -> [Stateful<Div>; 3] {
        let config = &self.config;
        let switches = [
            (
                "settings-confirm-close",
                "Confirm tab close",
                config.confirm_close_tab,
                Preference::ConfirmCloseTab(!config.confirm_close_tab),
            ),
            (
                "settings-confirm-close-pane",
                "Confirm pane close",
                config.confirm_close_pane,
                Preference::ConfirmClosePane(!config.confirm_close_pane),
            ),
            (
                "settings-confirm-close-workspace",
                "Confirm workspace close",
                config.confirm_close_workspace,
                Preference::ConfirmCloseWorkspace(!config.confirm_close_workspace),
            ),
        ];
        switches.map(|(id, label, on, edit)| self.preference_switch(id, label, on, edit, cx))
    }

    /// The automatic removal switch, with the note that says what it deletes.
    pub(super) fn merged_worktree_controls(&self, cx: &mut Context<Self>) -> [Div; 2] {
        let on = self.config.remove_merged_worktrees;
        [
            div().child(self.preference_switch(
                "settings-remove-merged-worktrees",
                "Remove worktrees of merged PRs",
                on,
                Preference::RemoveMergedWorktrees(!on),
                cx,
            )),
            self.control_note(
                "Deletes a clean linked worktree and closes its workspace once its PR is merged. \
                 The focused workspace asks first.",
            ),
        ]
    }

    fn preference_choice(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        selected: bool,
        edit: Preference,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.control_choice(id, label.into(), selected, true)
            .on_click(cx.listener(move |this, _, _, cx| this.save_preference(edit, cx)))
    }

    pub(super) fn native_notification_controls(&self, cx: &mut Context<Self>) -> Div {
        let config = self.config.notifications;
        let mut positions = div().flex().flex_wrap().gap(px(8.));
        for (label, position) in [
            ("Top left", Position::TopLeft),
            ("Top right", Position::TopRight),
            ("Bottom left", Position::BottomLeft),
            ("Bottom right", Position::BottomRight),
        ] {
            positions = positions.child(self.preference_choice(
                format!("notification-{label}"),
                label,
                config.position == position,
                Preference::NotificationPosition(Some(position)),
                cx,
            ));
        }
        self.control_card("Native notification overrides")
            .child(self.preference_switch("settings-notification-enabled", "In-app notifications", config.enabled, Preference::NotificationEnabled(Some(!config.enabled)), cx))
            .child(self.preference_choice("notification-follow-enabled", "Follow shared delivery", false, Preference::NotificationEnabled(None), cx))
            .child(self.preference_switch("settings-notification-finished", "Finished notifications", config.finished, Preference::NotificationFinished(!config.finished), cx))
            .child(self.control_row("Delay", format!("{} seconds (0-3600)", config.delay_seconds)))
            .child(div().flex().flex_wrap().gap(px(8.))
                .child(self.preference_choice("notification-delay-minus", "-1 second", false, Preference::NotificationDelay(Some(config.delay_seconds.saturating_sub(1))), cx))
                .child(self.preference_choice("notification-delay-plus", "+1 second", false, Preference::NotificationDelay(Some((config.delay_seconds + 1).min(3600))), cx))
                .child(self.preference_choice("notification-follow-delay", "Follow shared delay", false, Preference::NotificationDelay(None), cx)))
            .child(self.control_note("Position"))
            .child(positions)
            .child(self.preference_choice("notification-follow-position", "Follow shared position", false, Preference::NotificationPosition(None), cx))
            .child(self.control_note("Controls show effective values. Follow shared removes that local override; any managed-file override still takes precedence."))
    }

    /// The local `[sound] volume`, editable even where Herdr's shared sound
    /// settings are read-only, since it lives in this app's own file.
    pub(super) fn sound_volume_controls(&self, cx: &mut Context<Self>) -> Div {
        let volume = self.config.sound.volume;
        let mut levels = div().flex().flex_wrap().gap(px(8.));
        for percent in [10, 25, 50, 75, 100] {
            levels = levels.child(
                self.preference_choice(
                    format!("settings-sound-volume-{percent}"),
                    format!("{percent}%"),
                    volume == percent,
                    Preference::SoundVolume(percent),
                    cx,
                )
                .debug_selector(move || format!("settings-sound-volume-{percent}")),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(self.control_row("Volume", format!("{volume}%")))
            .child(levels)
    }

    pub(super) fn clipboard_controls(&self, cx: &mut Context<Self>) -> Div {
        let config = self.config.clipboard_toast;
        let mut positions = div().flex().flex_wrap().gap(px(8.));
        for (label, position) in [
            ("Top left", ClipboardPosition::TopLeft),
            ("Top center", ClipboardPosition::TopCenter),
            ("Top right", ClipboardPosition::TopRight),
            ("Bottom left", ClipboardPosition::BottomLeft),
            ("Bottom center", ClipboardPosition::BottomCenter),
            ("Bottom right", ClipboardPosition::BottomRight),
        ] {
            positions = positions.child(self.preference_choice(
                format!("clipboard-{label}"),
                label,
                config.position == position,
                Preference::ClipboardPosition(Some(position)),
                cx,
            ));
        }
        self.control_card("Clipboard feedback")
            .child(self.preference_switch(
                "settings-clipboard-enabled",
                "Copied notification",
                config.enabled,
                Preference::ClipboardEnabled(Some(!config.enabled)),
                cx,
            ))
            .child(self.preference_choice(
                "clipboard-follow-enabled",
                "Follow shared enablement",
                false,
                Preference::ClipboardEnabled(None),
                cx,
            ))
            .child(self.control_note("Position"))
            .child(positions)
            .child(self.preference_choice(
                "clipboard-follow-position",
                "Follow shared position",
                false,
                Preference::ClipboardPosition(None),
                cx,
            ))
    }

    /// One stepper per side. A step moves only its own side; the other side
    /// of the axis is saved along with it, since the file keeps them together.
    pub(in crate::settings_window) fn terminal_padding_controls(
        &self,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut card = self
            .control_card("Terminal padding")
            .mt(px(24.))
            .debug_selector(|| "settings-terminal-padding".into());
        for (axis, side, label, id) in [
            (Axis::X, Side::Start, "Left", "terminal-padding-left"),
            (Axis::X, Side::End, "Right", "terminal-padding-right"),
            (Axis::Y, Side::Start, "Top", "terminal-padding-top"),
            (Axis::Y, Side::End, "Bottom", "terminal-padding-bottom"),
        ] {
            let sides = self.config.terminal_padding.axis(axis);
            let value = format!("{} px (0-{MAX_PADDING})", sides.side(side));
            let mut steps = div().flex().gap(px(8.));
            for (step, label, suffix) in [(-1., "-1 px", "minus"), (1., "+1 px", "plus")] {
                steps = steps.child(
                    self.preference_choice(
                        format!("{id}-{suffix}"),
                        label,
                        false,
                        Preference::TerminalPadding(axis, sides.stepped(side, step)),
                        cx,
                    )
                    .debug_selector(move || format!("{id}-{suffix}")),
                );
            }
            card = card.child(self.control_row(label, value)).child(steps);
        }
        card
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use std::sync::{Arc, Mutex};

    #[gpui::test]
    fn switches_route_typed_edits_and_preserve_layout_drafts(cx: &mut TestAppContext) {
        let (view, cx) = cx.add_window_view(super::super::tests::skill_fixture);
        let edits = Arc::new(Mutex::new(Vec::new()));
        let captured = edits.clone();
        view.update(cx, |view, cx| {
            view.controls.preference_io = Some(PreferenceIo {
                write: Arc::new(move |edit| {
                    captured.lock().unwrap().push(edit);
                    Ok(())
                }),
                load: super::super::tests::skill_load,
            });
            view.accept_layout_choice(LayoutMode::Orca, cx);
        });
        cx.simulate_resize(size(px(960.), px(2200.)));
        for (section, selector, edit) in [
            (
                Section::General,
                "settings-confirm-close",
                Preference::ConfirmCloseTab(false),
            ),
            (
                Section::General,
                "settings-confirm-close-pane",
                Preference::ConfirmClosePane(false),
            ),
            (
                Section::General,
                "settings-smooth-scroll",
                Preference::SmoothScroll(false),
            ),
            (
                Section::General,
                "settings-confirm-close-workspace",
                Preference::ConfirmCloseWorkspace(false),
            ),
            (
                Section::General,
                "settings-remove-merged-worktrees",
                Preference::RemoveMergedWorktrees(true),
            ),
            (
                Section::General,
                "settings-clipboard-enabled",
                Preference::ClipboardEnabled(Some(false)),
            ),
            (
                Section::Notifications,
                "settings-notification-enabled",
                Preference::NotificationEnabled(Some(true)),
            ),
            (
                Section::Notifications,
                "settings-notification-finished",
                Preference::NotificationFinished(false),
            ),
        ] {
            view.update(cx, |view, cx| {
                view.section = section;
                cx.notify();
            });
            cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
            let point = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_click(point, Default::default());
            cx.run_until_parked();
            assert_eq!(edits.lock().unwrap().last(), Some(&edit));
            view.read_with(cx, |view, _| {
                assert!(!view.busy());
                assert_eq!(view.config.layout.mode, LayoutMode::Orca);
                assert_eq!(view.layout_intent, Some(LayoutMode::Orca));
            });
            let count = edits.lock().unwrap().len();
            view.update(cx, |view, cx| {
                view.saving = true;
                cx.notify();
            });
            cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
            let point = cx.debug_bounds(selector).unwrap().center();
            cx.simulate_click(point, Default::default());
            cx.run_until_parked();
            assert_eq!(edits.lock().unwrap().len(), count);
            view.update(cx, |view, _| view.saving = false);
        }
    }
}
