use super::*;
use crate::{config::DeviceSettings, sidebar::layout_tests::fixture_window};
use gpui::{Entity, TestAppContext, VisualTestContext, px, size};

const PROFILE: &str = "0123456789abcdef0123456789abcdef";
const HOST: &str = "ssh:0123456789abcdef0123456789abcdef";

fn builtin(name: &str) -> Theme {
    Theme::builtin(name).unwrap_or_default()
}

/// A window with Local and one saved device, the device selected.
fn open(cx: &mut TestAppContext) -> (Entity<HerdrWindow>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.endpoints[0].connection.target = herdr_client::ConnectTarget::Local;
        view.endpoints.push(crate::endpoint::Endpoint::new(
            HOST.into(),
            "box".into(),
            herdr_client::ConnectTarget::Ssh {
                target: "penso@box".into(),
                session: "default".into(),
            },
            true,
        ));
        view.selected_endpoint = 1;
        view.theme = builtin("Nord");
        view
    });
    cx.simulate_resize(size(px(800.), px(600.)));
    (view, cx)
}

fn set_device_theme(view: &Entity<HerdrWindow>, theme: Option<&str>, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        view.config.devices.insert(
            PROFILE.into(),
            DeviceSettings {
                theme: theme.map(str::to_owned),
                ..DeviceSettings::default()
            },
        );
    });
}

/// Draws, lets the background read finish, and draws what it applied.
fn settle(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn terminal_theme(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> Theme {
    view.read_with(cx, |view, _| view.terminal_theme().clone())
}

#[gpui::test]
fn the_selected_device_paints_its_terminal_with_its_own_theme(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    settle(cx);
    // No device theme: the terminal is the app theme.
    assert_eq!(terminal_theme(&view, cx), builtin("Nord"));

    set_device_theme(&view, Some("Dracula"), cx);
    settle(cx);
    assert_eq!(terminal_theme(&view, cx), builtin("Dracula"));
    view.read_with(cx, |view, _| {
        // The chrome keeps the app theme.
        assert_eq!(view.theme, builtin("Nord"));
        assert_eq!(view.device_themes.get(PROFILE), Some(&builtin("Dracula")));
    });

    // Local is not a saved device: selecting it shows the app theme.
    view.update(cx, |view, _| view.selected_endpoint = 0);
    assert_eq!(terminal_theme(&view, cx), builtin("Nord"));
    view.update(cx, |view, _| view.selected_endpoint = 1);
    assert_eq!(terminal_theme(&view, cx), builtin("Dracula"));

    // Removing the theme stops at once, before any read.
    set_device_theme(&view, None, cx);
    assert_eq!(terminal_theme(&view, cx), builtin("Nord"));
    settle(cx);
    assert_eq!(terminal_theme(&view, cx), builtin("Nord"));
}

#[gpui::test]
fn a_device_theme_that_cannot_load_keeps_the_app_theme(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    set_device_theme(&view, Some("../not-a-theme"), cx);
    settle(cx);
    assert_eq!(terminal_theme(&view, cx), builtin("Nord"));
    // Fixing it reads again.
    set_device_theme(&view, Some("Catppuccin Mocha"), cx);
    settle(cx);
    assert_eq!(terminal_theme(&view, cx), builtin("Catppuccin Mocha"));
}

#[gpui::test]
fn a_saved_choice_shows_until_the_reload_catches_up(cx: &mut TestAppContext) {
    let (view, cx) = open(cx);
    settle(cx);
    view.update(cx, |view, _| {
        view.pin_device_theme(Pinned {
            endpoint: HOST.into(),
            value: Some("Dracula".into()),
            theme: Some(builtin("Dracula")),
            revision: view.config_load_revision,
        });
    });
    // Before the reload, the config still says nothing.
    settle(cx);
    assert_eq!(terminal_theme(&view, cx), builtin("Dracula"));

    // The reload lands with the saved value; the pin goes once it is read.
    view.update(cx, |view, _| view.config_load_revision += 1);
    set_device_theme(&view, Some("Dracula"), cx);
    settle(cx);
    assert_eq!(terminal_theme(&view, cx), builtin("Dracula"));
    view.read_with(cx, |view, _| assert!(view.device_themes.pinned.is_none()));

    // A reload that does not hold the saved value, as after a failed save,
    // drops the pin rather than hiding what the file says.
    view.update(cx, |view, _| {
        view.pin_device_theme(Pinned {
            endpoint: HOST.into(),
            value: Some("Catppuccin Latte".into()),
            theme: Some(builtin("Catppuccin Latte")),
            revision: view.config_load_revision,
        });
        view.config_load_revision += 1;
    });
    settle(cx);
    assert_eq!(terminal_theme(&view, cx), builtin("Dracula"));
    view.read_with(cx, |view, _| assert!(view.device_themes.pinned.is_none()));
}
