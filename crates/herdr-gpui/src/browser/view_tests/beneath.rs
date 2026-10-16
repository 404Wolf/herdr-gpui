//! On macOS a page lies beneath the window's drawing: menus, tooltips,
//! toasts, and the file-transfer card draw over pages. So none of them hides
//! a page, and none holds frames back while it lays out.
use super::*;
use gpui::{Modifiers, px, size};
use herdr_client::protocol::SemanticNotificationKind;

#[gpui::test]
fn a_toast_shows_over_pages_without_hiding_them(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    cx.simulate_resize(size(px(1400.), px(800.)));
    draw(cx);
    view.update(cx, |view, cx| {
        view.config.notifications.delay_seconds = 3600;
        view.show_toast_preview(SemanticNotificationKind::Finished, cx);
    });
    draw(cx);
    draw(cx);
    view.read_with(cx, |view, _| {
        assert!(
            view.endpoints
                .iter()
                .any(|endpoint| !endpoint.toasts.entries.is_empty())
        );
        // Nothing records where the toast drew for pages to keep clear of.
        assert!(view.presented_overlays().is_empty());
        assert!(!view.pages_covered());
    });
}

#[gpui::test]
fn a_menu_opening_holds_no_frame_back(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    draw(cx);
    let button = cx.debug_bounds("tab-actions").unwrap();
    cx.simulate_click(button.center(), Modifiers::none());
    // Where pages draw above the window, a popover not yet laid out covers
    // an unknown area, and pages wait a frame to learn it.
    let settling = view.update(cx, |view, cx| view.present_browser(cx));
    assert!(view.read_with(cx, |view, _| view.menu.page.is_some()));
    assert!(!settling);
    assert!(!view.read_with(cx, |view, _| view.pages_covered()));
}

#[gpui::test]
fn hovering_the_status_bar_reserves_no_band(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    draw(cx);
    let bar = cx.debug_bounds("connection-status").unwrap();
    cx.simulate_mouse_move(bar.center(), None, Modifiers::default());
    draw(cx);
    assert!(
        view.read_with(cx, |view, _| view.browser.tooltip_band)
            .is_none()
    );
}
