#![allow(clippy::unwrap_used)]
use super::*;

fn view() -> Viewport {
    Viewport {
        pane_id: "p1".into(),
        terminal_id: "t1".into(),
        revision: 1,
        scroll: Some(PaneSurfaceScrollMetrics {
            offset_from_bottom: 0,
            max_offset_from_bottom: 20,
            viewport_rows: 24,
        }),
    }
}

#[test]
fn a_plain_live_hint_uses_only_a_verified_current_viewports_style() {
    let live = "header\n────────────\n❯\u{a0}Try a task\n────────────\nfooter\n";
    let colors =
        "header\n────────────\n❯\u{a0}\x1b[0m\x1b[2mTry a task\x1b[0m\n────────────\nfooter\n";
    assert!(!empty(live));
    assert!(verified(
        &view(),
        &view(),
        "p1",
        "t1",
        live.as_bytes(),
        colors.as_bytes(),
        live.as_bytes()
    ));
    // The same words typed by the user are not the dim placeholder.
    assert!(!verified(
        &view(),
        &view(),
        "p1",
        "t1",
        live.as_bytes(),
        live.as_bytes(),
        live.as_bytes()
    ));
    let mut scrolled = view();
    scrolled.scroll.as_mut().unwrap().offset_from_bottom = 10;
    assert!(!verified(
        &scrolled,
        &scrolled,
        "p1",
        "t1",
        live.as_bytes(),
        colors.as_bytes(),
        live.as_bytes()
    ));
    assert!(!verified(
        &view(),
        &scrolled,
        "p1",
        "t1",
        live.as_bytes(),
        colors.as_bytes(),
        live.as_bytes()
    ));
    let mut unknown = view();
    unknown.scroll = None;
    assert!(!verified(
        &unknown,
        &unknown,
        "p1",
        "t1",
        live.as_bytes(),
        colors.as_bytes(),
        live.as_bytes()
    ));
    assert!(!verified(
        &view(),
        &view(),
        "p2",
        "t1",
        live.as_bytes(),
        colors.as_bytes(),
        live.as_bytes()
    ));
    assert!(!verified(
        &view(),
        &view(),
        "p1",
        "t2",
        live.as_bytes(),
        colors.as_bytes(),
        live.as_bytes()
    ));
    let changed = live.replace("Try a task", "typed a draft");
    assert!(!verified(
        &view(),
        &view(),
        "p1",
        "t1",
        live.as_bytes(),
        colors.as_bytes(),
        changed.as_bytes()
    ));
}
