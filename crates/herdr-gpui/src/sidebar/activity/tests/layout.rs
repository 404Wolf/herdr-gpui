use super::*;
use crate::sidebar::layout_tests::{SidebarFixture, fixture_window, full_draw};
use gpui::{AppContext as _, Entity, Modifiers, VisualTestContext, px, size};

fn fixture(cx: &mut gpui::TestAppContext) -> (Entity<HerdrWindow>, &mut VisualTestContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        crate::bind_keys(cx);
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            view.config.sidebar_style.activity_bar = true;
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    cx.simulate_resize(size(px(1000.), px(700.)));
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        full_draw(window, cx).clear(cx);
    });
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx.debug_bounds(selector).unwrap();
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}

#[gpui::test]
fn the_bar_leads_the_expanded_sidebar_and_widens_its_column(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    let bar = cx.debug_bounds("activity-bar").unwrap();
    let sidebar = cx.debug_bounds("sidebar").unwrap();
    assert_eq!(bar.size.width, px(BAR_WIDTH));
    assert_eq!(bar.left(), px(0.));
    // The panel keeps its own width beside the bar rather than giving it up.
    assert_eq!(sidebar.left(), bar.right());
    let expected = cx.update(|_, cx| {
        let view = view.read(cx);
        crate::sidebar::sidebar_width(view.sidebar_width, 1000.) + BAR_WIDTH
    });
    assert_eq!(
        cx.update(|_, cx| view.read(cx).sidebar_column_width(1000.)),
        Some(expected)
    );
    // Settings moves to the bar; the footer keeps the picker and Sessions.
    assert!(cx.debug_bounds("activity-settings").is_some());
    assert!(cx.debug_bounds("device-settings").is_none());
    assert!(cx.debug_bounds("device-sessions").is_some());
    assert!(cx.debug_bounds("device-footer-initials").is_some());
}

#[gpui::test]
fn icons_switch_the_panel_and_the_shown_one_collapses_it(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    click(cx, "activity-pull-requests");
    assert_eq!(
        cx.update(|_, cx| view.read(cx).activity.panel),
        ActivityPanel::PullRequests
    );
    assert!(cx.debug_bounds("sidebar").is_none());
    let panel = cx.debug_bounds("activity-panel").unwrap();
    let bar = cx.debug_bounds("activity-bar").unwrap();
    assert_eq!(panel.left(), bar.right());
    assert!(cx.debug_bounds("activity-filter").is_some());
    assert!(cx.debug_bounds("device-footer").is_some());
    // No checkout has a pull request yet, and no orchestrator is open.
    assert!(cx.debug_bounds("activity-empty").is_some());
    click(cx, "activity-issues");
    assert!(cx.debug_bounds("activity-open-orchestrator").is_some());
    // The overview is not a destination yet.
    click(cx, "activity-overview");
    assert_eq!(
        cx.update(|_, cx| view.read(cx).activity.panel),
        ActivityPanel::Issues
    );
    click(cx, "activity-issues");
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(!view.sidebar_visible);
        assert_eq!(view.activity.panel, ActivityPanel::Issues);
    });
    // The collapsed rail keeps no bar.
    assert!(cx.debug_bounds("activity-bar").is_none());
}

#[gpui::test]
fn a_seeded_pull_request_is_listed_and_counted(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    view.update(cx, |view, cx| {
        let mut value = crate::pull_request::fixture().unwrap();
        value.title = "Seeded pull request".into();
        view.menu.pr_cache.seed(
            crate::pull_request::Input {
                checkout: None,
                repo_key: Some(crate::sidebar::layout_tests::REPO_KEY.into()),
                branch: "develop".into(),
            },
            value,
            std::time::Instant::now(),
        );
        view.show_activity_panel(ActivityPanel::PullRequests, cx);
    });
    draw(cx);
    assert!(cx.debug_bounds("activity-row-0-0").is_some());
    assert!(cx.debug_bounds("activity-row-0-1").is_none());
    let lists = cx.update(|_, cx| view.read(cx).activity_lists(cx));
    assert_eq!(lists.count(ActivityPanel::PullRequests), Some(1));
    assert_eq!(lists.count(ActivityPanel::Workspaces), None);
}

#[gpui::test]
fn turning_the_bar_off_restores_the_plain_sidebar(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    view.update(cx, |view, cx| {
        view.config.sidebar_style.activity_bar = false;
        cx.notify();
    });
    draw(cx);
    assert!(cx.debug_bounds("activity-bar").is_none());
    assert_eq!(cx.debug_bounds("sidebar").unwrap().left(), px(0.));
    assert!(cx.debug_bounds("device-settings").is_some());
    let width = cx.update(|_, cx| view.read(cx).sidebar_column_width(1000.));
    assert_eq!(width, Some(crate::sidebar::sidebar_width(None, 1000.)));
}

#[test]
fn panel_names_round_trip_and_are_unique() {
    let mut seen = std::collections::HashSet::new();
    for &panel in ActivityPanel::ALL {
        assert!(seen.insert(panel.name()));
        assert_eq!(ActivityPanel::parse(panel.name()), Some(panel));
        let json = serde_json::to_string(&panel).unwrap();
        assert_eq!(json, format!("\"{}\"", panel.name()));
    }
    assert_eq!(ActivityPanel::parse("overview"), None);
}

#[gpui::test]
fn a_stored_panel_yields_to_one_picked_before_it_loaded(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    view.update(cx, |view, _| {
        assert_eq!(view.activity.chosen(), None);
        view.activity.restore(Some(ActivityPanel::Orchestrator));
        assert_eq!(view.activity.panel, ActivityPanel::Orchestrator);
    });
    let (view, cx) = fixture(cx);
    view.update(cx, |view, cx| {
        view.show_activity_panel(ActivityPanel::Issues, cx);
        view.activity.restore(Some(ActivityPanel::Orchestrator));
        assert_eq!(view.activity.panel, ActivityPanel::Issues);
        assert_eq!(view.activity.chosen(), Some(ActivityPanel::Issues));
    });
}
