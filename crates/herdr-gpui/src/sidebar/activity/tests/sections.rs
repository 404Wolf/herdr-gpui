//! A section shows the focused workspace's orchestrator tab, opening one as
//! the workspace menu would when there is none.
use super::*;
use crate::{
    browser::{Location, Store},
    orchestrator::List,
    sidebar::layout_tests::{SidebarFixture, fixture_window, full_draw, snapshot},
    state::ConnectionStatus,
};
use gpui::{AppContext as _, Entity, VisualTestContext, px, size};
use herdr_client::ConnectTarget;
use std::sync::Arc;

fn local(cx: &mut gpui::TestAppContext) -> (Entity<HerdrWindow>, &mut VisualTestContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            view.config.sidebar_style.activity_bar = true;
            // The local session can run the orchestrator's scripts.
            view.endpoints[0].connection.target = ConnectTarget::Local;
            let mut listing = snapshot(2);
            listing.focused_workspace_id = Some("w0".into());
            view.live.snapshot = Some(Arc::new(listing));
            view.live.status = ConnectionStatus::Connected;
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    cx.simulate_resize(size(px(1000.), px(700.)));
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    (view, cx)
}

fn show(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext, panel: ActivityPanel) {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| view.show_activity_panel(panel, window, cx));
        full_draw(window, cx).clear(cx);
    });
}

fn orchestrator_tabs(cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| {
        cx.try_global::<Store>().map_or(0, |store| {
            store
                .tabs()
                .filter(|tab| matches!(tab.location, Some(Location::Orchestrator { .. })))
                .count()
        })
    })
}

fn shown_list(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> List {
    cx.update(|_, cx| {
        let view = view.read(cx);
        let id = view.activity.section_tab.unwrap();
        view.orchestrators[&id].view().read(cx).list()
    })
}

#[gpui::test]
fn a_section_opens_the_orchestrator_once_and_picks_its_list(cx: &mut gpui::TestAppContext) {
    let (view, cx) = local(cx);
    assert_eq!(orchestrator_tabs(cx), 0);
    show(&view, cx, ActivityPanel::Issues);
    assert_eq!(orchestrator_tabs(cx), 1);
    assert_eq!(shown_list(&view, cx), List::Issues);
    // The tab's view fills the section, holding the keyboard.
    assert!(cx.debug_bounds("activity-section").is_some());
    assert!(cx.debug_bounds("activity-section-empty").is_none());
    cx.update(|window, cx| {
        let view = view.read(cx);
        let id = view.activity.section_tab.unwrap();
        let focus = view.orchestrators[&id]
            .view()
            .read(cx)
            .focus_handle()
            .clone();
        assert!(focus.contains_focused(window, cx));
    });
    // Another section reuses the tab, on its own list.
    show(&view, cx, ActivityPanel::PullRequests);
    assert_eq!(orchestrator_tabs(cx), 1);
    assert_eq!(shown_list(&view, cx), List::PullRequests);
    show(&view, cx, ActivityPanel::Orchestrator);
    assert_eq!(shown_list(&view, cx), List::Runs);
    // The tick keeps the section where it is.
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll_orchestrators(window, cx)));
    assert_eq!(orchestrator_tabs(cx), 1);
}

#[gpui::test]
fn a_section_follows_the_focused_workspace(cx: &mut gpui::TestAppContext) {
    let (view, cx) = local(cx);
    show(&view, cx, ActivityPanel::Orchestrator);
    let first = cx.update(|_, cx| view.read(cx).activity.section_tab);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            if let Some(snapshot) = view.live.snapshot.as_mut() {
                Arc::make_mut(snapshot).focused_workspace_id = Some("w1".into());
            }
            view.poll_orchestrators(window, cx);
        })
    });
    let second = cx.update(|_, cx| view.read(cx).activity.section_tab);
    assert_ne!(first, second);
    assert_eq!(orchestrator_tabs(cx), 2);
    let workspace = cx.update(|_, cx| {
        cx.try_global::<Store>()
            .and_then(|store| store.get(second?))
            .map(|tab| tab.workspace_id.clone())
    });
    assert_eq!(workspace.as_deref(), Some("w1"));
}
