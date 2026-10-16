use super::*;
use crate::config::{StatusIcons, preferences::Preference};
use crate::settings_window::controls::preferences::PreferenceIo;
use std::sync::{Arc, Mutex};

#[gpui::test]
fn indicators_offer_each_icon_set_and_save_only_a_change(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(skill_fixture);
    let edits = Arc::new(Mutex::new(Vec::new()));
    let captured = edits.clone();
    view.update(cx, |view, cx| {
        view.controls.preference_io = Some(PreferenceIo {
            write: Arc::new(move |edit| {
                captured.lock().unwrap().push(edit);
                Ok(())
            }),
            load: skill_load,
        });
        view.section = Section::Indicators;
        cx.notify();
    });
    cx.simulate_resize(size(px(960.), px(2200.)));
    for (icons, id) in [
        (StatusIcons::Solid, "settings-indicators-solid"),
        (StatusIcons::Tiles, "settings-indicators-tiles"),
        (StatusIcons::Rail, "settings-indicators-rail"),
    ] {
        cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
        // Drawn icons are this client's own setting, so they stay available
        // even when Herdr's shared settings are not.
        let choice = cx.debug_bounds(id).unwrap();
        cx.simulate_click(choice.center(), Default::default());
        cx.run_until_parked();
        assert_eq!(
            edits.lock().unwrap().pop(),
            Some(Preference::StatusIcons(Some(icons)))
        );
        view.update(cx, |view, cx| {
            view.config.status_icons = Some(icons);
            cx.notify();
        });
        cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
        let choice = cx.debug_bounds(id).unwrap();
        cx.simulate_click(choice.center(), Default::default());
        cx.run_until_parked();
        assert!(edits.lock().unwrap().is_empty(), "{id} saved again");
    }
}

/// Where Herdr's shared settings cannot be written, as on Windows or when
/// they failed to load, the style Herdr already uses still undoes an icon
/// set: it only drops this client's key. The other style stays disabled.
#[gpui::test]
fn herdr_style_undoes_icons_without_writable_shared_settings(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(skill_fixture);
    let edits = Arc::new(Mutex::new(Vec::new()));
    let captured = edits.clone();
    view.update(cx, |view, cx| {
        assert!(!view.controls_shared_ready());
        view.controls.preference_io = Some(PreferenceIo {
            write: Arc::new(move |edit| {
                captured.lock().unwrap().push(edit);
                Ok(())
            }),
            load: skill_load,
        });
        view.config.status_icons = Some(StatusIcons::Tiles);
        view.section = Section::Indicators;
        cx.notify();
    });
    cx.simulate_resize(size(px(960.), px(2200.)));
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    // Unreadable shared settings leave Herdr's default, Dots, in charge.
    let symbols = cx.debug_bounds("settings-indicators-Symbols").unwrap();
    cx.simulate_click(symbols.center(), Default::default());
    cx.run_until_parked();
    assert!(edits.lock().unwrap().is_empty());
    let dots = cx.debug_bounds("settings-indicators-Dots").unwrap();
    cx.simulate_click(dots.center(), Default::default());
    cx.run_until_parked();
    assert_eq!(
        edits.lock().unwrap().as_slice(),
        [Preference::StatusIcons(None)]
    );
}
