//! A tab carried out of the window and dropped there opens in a new window.
//! Tests open no windows: each tear-off is recorded instead.
use {
    super::*,
    crate::{
        browser::{Pick, Store, TabId},
        window::tab_tear::{TornOff, TornTab, remote_session},
    },
};

/// Past the bottom right of the 1600 by 600 window.
const OUTSIDE: Point<Pixels> = point(px(1800.), px(900.));

fn torn(cx: &mut VisualTestContext) -> Vec<TornTab> {
    cx.update(|_, cx| {
        cx.try_global::<TornOff>()
            .map(|torn| torn.0.clone())
            .unwrap_or_default()
    })
}

/// Carries the tab drawn as `tab` to `to`, and lets go there.
fn carry_to(cx: &mut VisualTestContext, tab: &str, to: Point<Pixels>) {
    let tab: &'static str = Box::leak(tab.to_owned().into_boxed_str());
    let from = cx.debug_bounds(tab).unwrap().center();
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    for _ in 0..2 {
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        draw(cx);
    }
}

fn drop_at(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
    draw(cx);
}

fn primary(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> Option<Pick> {
    view.read_with(cx, |view, _| view.group_pick(view.group_slots()[0].id))
}

#[gpui::test]
fn a_tab_dropped_outside_the_window_opens_in_a_new_one(cx: &mut TestAppContext) {
    let (view, cx) = four_tabs(cx, true);
    carry_to(cx, "tab-c", OUTSIDE);
    // Out of the window it takes no place in its strip.
    assert_eq!(target(&view, cx), None);
    assert!(torn(cx).is_empty());
    drop_at(cx, OUTSIDE);

    let torn = torn(cx);
    assert_eq!(torn.len(), 1);
    assert_eq!(torn[0].pick, Pick::Herdr("c".into()));
    // The new window opens on the same display, carried as far as the tab.
    let (bounds, _) = torn[0].placement;
    assert!(bounds.size.width >= px(640.) && bounds.size.height >= px(400.));
    view.read_with(cx, |view, _| {
        assert!(view.tab_drag.is_none());
        // Every window lists the workspace's tabs; nothing was moved.
        assert!(view.live.snapshot.as_ref().unwrap().tabs.len() == 4);
    });
    // The group showed another tab, which it keeps showing.
    assert_eq!(primary(&view, cx), Some(Pick::Herdr("a".into())));
}

#[gpui::test]
fn the_group_moves_on_when_the_tab_it_shows_is_torn_off(cx: &mut TestAppContext) {
    let (view, cx) = four_tabs(cx, true);
    carry_to(cx, "tab-a", OUTSIDE);
    drop_at(cx, OUTSIDE);
    assert_eq!(torn(cx)[0].pick, Pick::Herdr("a".into()));
    assert_eq!(primary(&view, cx), Some(Pick::Herdr("b".into())));
}

#[gpui::test]
fn a_tab_dropped_inside_the_window_tears_nothing_off(cx: &mut TestAppContext) {
    let (view, cx) = four_tabs(cx, true);
    let inside = point(px(800.), px(400.));
    carry_to(cx, "tab-b", inside);
    drop_at(cx, inside);
    assert!(torn(cx).is_empty());
    assert_eq!(primary(&view, cx), Some(Pick::Herdr("a".into())));
}

/// A lone tab, which nothing in its strip could be reordered with, and a
/// daemon that cannot move tabs, can still be carried off.
#[gpui::test]
fn a_lone_tab_can_be_torn_off(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = crate::sidebar::layout_tests::fixture_window(window, cx);
        let snapshot: herdr_client::protocol::ClientShellSnapshot = serde_json::from_str(
            include_str!("../../../../../herdr-protocol/tests/fixtures/endpoint-snapshot-v1.json"),
        )
        .unwrap();
        view.live.snapshot = Some(Arc::new(snapshot));
        view.live.status = crate::state::ConnectionStatus::Connected;
        view
    });
    cx.simulate_resize(size(px(1600.), px(600.)));
    for _ in 0..2 {
        draw(cx);
    }
    carry_to(cx, "tab-w1:t1", OUTSIDE);
    drop_at(cx, OUTSIDE);
    assert_eq!(
        torn(cx).iter().map(|torn| &torn.pick).collect::<Vec<_>>(),
        [&Pick::Herdr("w1:t1".into())]
    );
    assert_eq!(primary(&view, cx), Some(Pick::Herdr("w1:t1".into())));
}

fn open_page(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> TabId {
    cx.update(|_, cx| {
        let scope = crate::browser::scope(&view.read(cx).endpoints[0]);
        Store::update(cx, |store| store.open(scope, "w1", None, None)).unwrap()
    })
}

#[gpui::test]
fn a_page_tab_can_be_torn_off(cx: &mut TestAppContext) {
    let (view, cx) = four_tabs(cx, true);
    let page = open_page(&view, cx);
    cx.update(|window, cx| view.update(cx, |view, cx| view.show_browser_tab(page, window, cx)));
    for _ in 0..2 {
        draw(cx);
    }
    carry_to(cx, &format!("browser-tab-{page}"), OUTSIDE);
    drop_at(cx, OUTSIDE);
    assert_eq!(torn(cx)[0].pick, Pick::Page(page));
    // The page is still this client's, listed in every window.
    cx.update(|_, cx| assert!(cx.global::<Store>().get(page).is_some()));
    assert_eq!(primary(&view, cx), Some(Pick::Herdr("d".into())));
}

/// The window opened for a torn-off tab is not connected yet, so it goes
/// to the tab once its surface is ready.
#[gpui::test]
fn a_new_window_goes_to_the_tab_torn_off_once_connected(cx: &mut TestAppContext) {
    let (source, cx) = four_tabs(cx, true);
    carry_to(cx, "tab-c", OUTSIDE);
    drop_at(cx, OUTSIDE);
    let herdr = torn(cx).remove(0);
    let page = open_page(&source, cx);
    let mut page_torn = herdr.clone();
    page_torn.pick = Pick::Page(page);

    let fresh = cx
        .update(|window, cx| cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)));
    cx.update(|window, cx| fresh.update(cx, |view, cx| view.arrive(herdr.clone(), window, cx)));
    fresh.read_with(cx, |view, _| {
        assert_eq!(
            view.pending_navigation,
            Some(crate::NavigationTarget::Tab("c".into()))
        );
    });
    cx.update(|window, cx| fresh.update(cx, |view, cx| view.arrive(page_torn, window, cx)));
    fresh.read_with(cx, |view, _| {
        assert_eq!(
            view.pending_navigation,
            Some(crate::NavigationTarget::Workspace("w1".into()))
        );
    });
}

fn saved(id: &str) -> herdr_client::SavedHost {
    herdr_client::SavedHost {
        id: id.into(),
        label: id.into(),
        target: format!("user@{id}"),
        session: "default".into(),
        enabled: true,
    }
}

/// A new window lists only this machine's daemon until it reads the saved
/// hosts, so a tab torn off a remote host waits for that host.
#[gpui::test]
fn a_tab_from_a_saved_host_waits_for_the_host_to_be_listed(cx: &mut TestAppContext) {
    let (_source, cx) = four_tabs(cx, true);
    carry_to(cx, "tab-c", OUTSIDE);
    drop_at(cx, OUTSIDE);
    let mut torn = torn(cx).remove(0);
    torn.endpoint = "ssh:remote".into();
    torn.session = herdr_client::ConnectTarget::Ssh {
        target: "user@remote".into(),
        session: "default".into(),
    };

    let fresh = cx
        .update(|window, cx| cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)));
    cx.update(|window, cx| fresh.update(cx, |view, cx| view.arrive(torn.clone(), window, cx)));
    fresh.read_with(cx, |view, _| {
        assert!(view.arrival.is_some());
        assert_eq!(view.pending_navigation, None);
        assert_eq!(view.selected_endpoint, 0);
    });

    // Listed, the host is chosen and the tab is gone to once connected.
    fresh.update(cx, |view, cx| {
        view.reconcile_catalog(vec![saved("remote")], Vec::new(), cx)
    });
    fresh.read_with(cx, |view, _| {
        assert!(view.arrival.is_none());
        assert_eq!(view.endpoints[view.selected_endpoint].id, "ssh:remote");
        assert_eq!(
            view.pending_navigation,
            Some(crate::NavigationTarget::Tab("c".into()))
        );
    });
}

#[gpui::test]
fn a_tab_from_a_host_no_longer_saved_is_given_up(cx: &mut TestAppContext) {
    let (_source, cx) = four_tabs(cx, true);
    carry_to(cx, "tab-c", OUTSIDE);
    drop_at(cx, OUTSIDE);
    let mut torn = torn(cx).remove(0);
    torn.endpoint = "ssh:gone".into();
    let fresh = cx
        .update(|window, cx| cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)));
    cx.update(|window, cx| fresh.update(cx, |view, cx| view.arrive(torn, window, cx)));
    fresh.update(cx, |view, cx| {
        view.reconcile_catalog(vec![saved("remote")], Vec::new(), cx)
    });
    fresh.read_with(cx, |view, _| {
        assert!(view.arrival.is_none());
        assert_eq!(view.selected_endpoint, 0);
        assert_eq!(view.pending_navigation, None);
    });
}

#[test]
fn only_a_remote_host_names_a_session_to_restore() {
    use herdr_client::ConnectTarget;
    let ssh = ConnectTarget::Ssh {
        target: "user@host".into(),
        session: "work".into(),
    };
    let wsl = ConnectTarget::Wsl {
        distro: "Ubuntu".into(),
        session: "dev".into(),
    };
    assert_eq!(remote_session(&ssh), Some("work"));
    assert_eq!(remote_session(&wsl), Some("dev"));
    assert_eq!(remote_session(&ConnectTarget::Local), None);
}
