use super::*;

#[gpui::test]
fn coder_row_appears_only_when_configured_and_its_dialog_fits(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        crate::bind_keys(cx);
        let view = cx.new(|cx| fixture_window(window, cx));
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    cx.simulate_resize(size(px(800.), px(600.)));
    cx.run_until_parked();
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    let picker = cx.debug_bounds("device-picker").unwrap().center();
    cx.simulate_click(picker, Modifiers::default());
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    let rows = view.read_with(cx, |view, _| view.endpoints.len() + 2);
    // Debug selectors are looked up by static name. Configured providers follow
    // the saved devices, with Coder first when it is configured.
    let row = |index: usize| -> &'static str {
        Box::leak(format!("device-row-{index}").into_boxed_str())
    };
    let coder_row = row(rows);
    // Providers that need no account (Railway) are always offered; the rest
    // follow their configuration. Count the provider rows rather than assume
    // which one comes first.
    let offered = |cx: &mut gpui::VisualTestContext| {
        (rows..rows + 8)
            .take_while(|&index| cx.debug_bounds(row(index)).is_some())
            .count()
    };
    let configured_elsewhere = std::env::var_os("HERDR_CODER_URL").is_some()
        || std::env::var_os("HERDR_DAYTONA_API_URL").is_some();
    let before = offered(cx);
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| {
        view.update(cx, |view, _| {
            view.config.coder.url = Some("https://coder.example.com".into());
        });
        full_draw(window, cx).clear(cx);
    });
    cx.simulate_click(picker, Modifiers::default());
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    if crate::cloud::unavailable().is_some() {
        // Nothing offers to create a device that could never connect.
        assert!(cx.debug_bounds(row(rows)).is_none());
        return;
    }
    assert!(cx.debug_bounds(coder_row).is_some());
    if !configured_elsewhere {
        assert_eq!(offered(cx), before + 1, "configuring Coder adds its row");
    }
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.open_coder_fixture(window, cx));
        full_draw(window, cx).clear(cx);
    });
    for (width, height) in [(320., 300.), (800., 600.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.update(|window, cx| full_draw(window, cx).clear(cx));
        let panel = cx.debug_bounds("menu-panel").unwrap();
        let dialog = cx.debug_bounds("coder-setup-dialog").unwrap();
        let submit = cx.debug_bounds("coder-setup-submit").unwrap();
        assert!(dialog.right() <= panel.right() && dialog.left() >= panel.left());
        assert!(submit.bottom() <= panel.bottom());
        assert!(panel.bottom() <= px(height));
        assert!(panel.right() <= px(width));
    }
    // Typing stays in the dialog; shortcuts cannot reach the terminal behind it.
    cx.simulate_keystrokes("cmd-b");
    cx.update(|_, cx| assert!(view.read(cx).sidebar_visible));
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| {
        assert!(view.read(cx).menu.page.is_none());
        assert!(view.read(cx).focus.is_focused(window));
    });
}
