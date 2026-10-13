use super::*;
use crate::config::DeviceFooter;

fn set_footer(view: &Entity<HerdrWindow>, mode: DeviceFooter, cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| {
        view.update(cx, |view, _| view.config.sidebar_style.device_footer = mode);
        full_draw(window, cx).clear(cx);
    });
}

#[gpui::test]
fn device_footer_setting_hides_it_and_gives_agents_the_room(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    let sidebar = cx.debug_bounds("sidebar").unwrap();
    let shown = cx.debug_bounds("agents-section").unwrap();
    assert!(cx.debug_bounds("device-footer").is_some());

    // The fixture has one device, so auto hides the footer like never does.
    for mode in [DeviceFooter::Auto, DeviceFooter::Never] {
        set_footer(&view, mode, cx);
        assert!(cx.debug_bounds("device-footer").is_none(), "{mode:?}");
        let agents = cx.debug_bounds("agents-section").unwrap();
        assert_eq!(agents.bottom(), sidebar.bottom());
        assert!(agents.size.height > shown.size.height);
    }

    // The shortcut still opens Sessions, above the sidebar's bottom-left corner.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.command(crate::controls::Command::Sessions, window, cx);
        });
        full_draw(window, cx).clear(cx);
    });
    let menu = cx.debug_bounds("menu-panel").unwrap();
    assert_eq!(
        view.read_with(cx, |view, _| view.menu.page),
        Some(crate::menu::Page::Sessions)
    );
    assert!(menu.bottom() < sidebar.bottom());
    assert!(menu.top() > sidebar.top());
    assert_eq!(menu.left(), sidebar.left() + px(8.));
    cx.simulate_keystrokes("escape");

    // A second device brings the footer back under auto, not under never.
    view.update(cx, |view, _| {
        let mut remote = crate::endpoint::Endpoint::new(
            "ssh:fixture".into(),
            "Remote".into(),
            ConnectTarget::Ssh {
                target: "example.invalid".into(),
                session: "default".into(),
            },
            true,
        );
        remote.live.snapshot = view.live.snapshot.clone();
        view.endpoints.push(remote);
    });
    set_footer(&view, DeviceFooter::Never, cx);
    assert!(cx.debug_bounds("device-footer").is_none());
    set_footer(&view, DeviceFooter::Auto, cx);
    let footer = cx.debug_bounds("device-footer").unwrap();
    assert_eq!(footer.bottom(), sidebar.bottom());
    assert!(cx.debug_bounds("agents-section").unwrap().bottom() <= footer.top());
}
