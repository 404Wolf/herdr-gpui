use super::*;
use crate::config::{Feature, preferences::Preference};
use crate::settings_window::controls::preferences::PreferenceIo;
use std::sync::{Arc, Mutex};

#[gpui::test]
fn every_feature_has_a_switch_that_saves_the_opposite(cx: &mut TestAppContext) {
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
        view.section = Section::General;
        cx.notify();
    });
    cx.simulate_resize(size(px(960.), px(2200.)));
    for &feature in Feature::ALL {
        cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
        // Debug selectors are looked up by static name.
        let selector = format!("settings-feature-{}", feature.key()).leak();
        let switch = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(switch.center(), Default::default());
        cx.run_until_parked();
        assert_eq!(
            edits.lock().unwrap().pop(),
            Some(Preference::Feature(feature, true))
        );
    }
    view.update(cx, |view, cx| {
        view.config.features.sidebar_hover_menu = true;
        cx.notify();
    });
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    let switch = cx
        .debug_bounds("settings-feature-sidebar_hover_menu")
        .unwrap();
    cx.simulate_click(switch.center(), Default::default());
    cx.run_until_parked();
    assert_eq!(
        edits.lock().unwrap().pop(),
        Some(Preference::Feature(Feature::SidebarHoverMenu, false))
    );
}
