//! General > Startup's **Launch at login** row, on Windows. The registry is
//! the setting (`crate::launch_at_login`), read when the window opens and
//! again after each change, so the row shows what Windows will do.
use super::{SettingsWindow, ui};
use crate::launch_at_login::{self, State};
use gpui::{prelude::*, *};

impl SettingsWindow {
    /// Reads the startup entry off the UI thread, first writing it when
    /// `change` is `Some(on)`. The toggle stays disabled until it is read.
    pub(super) fn refresh_launch_at_login(&mut self, change: Option<bool>, cx: &mut Context<Self>) {
        self.launch_at_login = None;
        let work = cx.background_executor().spawn(async move {
            let saved = change.map_or(Ok(()), launch_at_login::set);
            (saved, launch_at_login::state())
        });
        cx.spawn(async move |this, cx| {
            let (saved, state) = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = &saved {
                    tracing::warn!(%error, "Could not change launch at login");
                    this.error = Some(format!("Launch at login: {error}"));
                }
                match state {
                    Ok(state) => this.launch_at_login = Some(state),
                    Err(error) => {
                        tracing::warn!(%error, "Could not read launch at login");
                        if saved.is_ok() {
                            this.error = Some(format!("Launch at login: {error}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn launch_at_login_rows(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = &self.theme;
        let state = self.launch_at_login;
        let on = state == Some(State::On);
        let ready = matches!(state, Some(State::On | State::Off));
        let row = ui::row(theme, "general-launch-at-login", "Launch at login")
            .description("Open Herdr UI when you sign in to Windows.");
        // Windows' own switch wins; the app never clears its mark.
        let row = if state == Some(State::DisabledByWindows) {
            row.disabled("Turned off in Settings > Apps > Startup. Turn it back on there.")
        } else {
            row
        };
        let toggle =
            ui::toggle(theme, "general-launch-at-login-toggle", on, ready).when(ready, |toggle| {
                toggle.on_click(
                    cx.listener(move |this, _, _, cx| this.refresh_launch_at_login(Some(!on), cx)),
                )
            });
        vec![
            ui::section(theme, "Startup", None).into_any_element(),
            row.control(toggle).into_any_element(),
        ]
    }
}
