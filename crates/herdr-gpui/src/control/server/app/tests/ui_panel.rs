//! `ui panel` against a headless window.
use super::*;
use crate::sidebar::{ActivityPanel, layout_tests::fixture_window};

#[gpui::test]
fn ui_panel_shows_a_section_and_workspaces_expands_the_sidebar(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.sidebar_style.activity_bar = true;
        view.sidebar_visible = false;
        view
    });
    let shown = cx.cx.update(|cx| {
        ui(
            call(UiOp::Panel {
                panel: ActivityPanel::Issues,
            }),
            cx,
        )
    });
    assert!(matches!(shown, Response::Done { .. }), "{shown:?}");
    cx.cx.update(|cx| {
        let view = view.read(cx);
        assert_eq!(view.activity.panel, ActivityPanel::Issues);
        assert!(view.activity_section().is_some());
    });
    // Workspaces brings the sidebar back expanded.
    let shown = cx.cx.update(|cx| {
        ui(
            call(UiOp::Panel {
                panel: ActivityPanel::Workspaces,
            }),
            cx,
        )
    });
    assert!(matches!(shown, Response::Done { .. }), "{shown:?}");
    assert!(view.read_with(cx, |view, _| view.sidebar_visible));
    // Without the bar there is no panel to show, which is said, not faked.
    view.update(cx, |view, _| view.config.sidebar_style.activity_bar = false);
    let refused = cx.cx.update(|cx| {
        ui(
            call(UiOp::Panel {
                panel: ActivityPanel::Workspaces,
            }),
            cx,
        )
    });
    assert_eq!(code(&refused), Some(ErrorCode::Unsupported));
    assert_eq!(
        view.read_with(cx, |view, _| view.activity.panel),
        ActivityPanel::Workspaces
    );
}
