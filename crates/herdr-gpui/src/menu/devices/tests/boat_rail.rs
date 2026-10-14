use super::*;
use crate::cloud::CloudProvider;
use gpui::{Modifiers, px, size};

#[gpui::test]
fn boat_in_the_add_device_rail_opens_its_settings(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(900.), px(700.)));
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_add_device(window, cx)));
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    if crate::cloud::unavailable().is_some() {
        assert!(cx.debug_bounds("add-device-source-boat").is_none());
        return;
    }
    let target = cx.debug_bounds("add-device-source-boat").unwrap().center();
    cx.simulate_click(target, Modifiers::default());
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).menu.page,
            Some(Page::AddCloud(CloudProvider::Boat))
        )
    });
    let target = cx.debug_bounds("cloud-setup-settings").unwrap().center();
    cx.simulate_click(target, Modifiers::default());
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).menu.settings_opened,
            Some(CloudProvider::Boat)
        )
    });
}
