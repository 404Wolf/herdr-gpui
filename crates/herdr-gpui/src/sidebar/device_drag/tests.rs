#![allow(clippy::unwrap_used)]
// Named rather than globbed: the parent's GPUI glob would shadow `#[test]`.
use super::landing;
use crate::{
    HerdrWindow,
    reorder::{Beside, LIFT_DELAY},
    sidebar::layout_tests::{fixture_window, full_draw, snapshot},
};
use gpui::{Entity, Modifiers, MouseButton, VisualTestContext, point, px, size};
use herdr_client::ConnectTarget;
use std::sync::Arc;

#[test]
fn a_drop_lands_before_the_device_holding_the_gap_or_after_the_last() {
    let ids: Vec<String> = ["a", "b", "c"].map(String::from).into();
    // The gaps around the carried device move nothing.
    assert_eq!(landing(&ids, 1, 1), None);
    assert_eq!(landing(&ids, 1, 2), None);
    assert_eq!(landing(&ids, 1, 0), Some(Beside::Before("a".into())));
    assert_eq!(landing(&ids, 0, 2), Some(Beside::Before("c".into())));
    assert_eq!(landing(&ids, 0, 3), Some(Beside::After("c".into())));
}

fn remote(id: &str) -> crate::endpoint::Endpoint {
    let mut endpoint = crate::endpoint::Endpoint::new(
        format!("ssh:{id}"),
        id.to_uppercase(),
        ConnectTarget::Ssh {
            target: id.into(),
            session: "default".into(),
        },
        true,
    );
    endpoint.live.snapshot = Some(Arc::new(snapshot(3)));
    endpoint
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
}

fn ids(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| {
        view.endpoints.iter().map(|e| e.id.clone()).collect()
    })
}

#[gpui::test]
fn a_held_device_header_folds_the_list_and_drops_into_the_gap(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.live.snapshot = Some(Arc::new(snapshot(3)));
        view.endpoints
            .extend(["a", "b", "c"].into_iter().map(remote));
        view
    });
    cx.simulate_resize(size(px(800.), px(900.)));
    draw(cx);
    let header = cx.debug_bounds("host-ssh:a").unwrap();
    assert!(cx.debug_bounds("workspace-ssh:a-w0").is_some());

    // Held, a device's header lifts and the list folds to the headers.
    cx.simulate_mouse_down(header.center(), MouseButton::Left, Modifiers::default());
    cx.executor().advance_clock(LIFT_DELAY);
    cx.run_until_parked();
    draw(cx);
    draw(cx);
    assert!(cx.debug_bounds("workspace-ssh:a-w0").is_none());
    assert!(cx.debug_bounds("workspace-local-w0").is_none());
    // The lifted header stays under the pointer, which has not moved, while
    // the others rest one after another, its own place open above them.
    let carried = cx.debug_bounds("host-ssh:a").unwrap();
    assert_eq!(carried.top(), header.top());
    let b = cx.debug_bounds("host-ssh:b").unwrap();
    let c = cx.debug_bounds("host-ssh:c").unwrap();
    assert_eq!(c.top(), b.bottom());
    let rest = b.top() - carried.size.height;
    let target = |view: &Entity<HerdrWindow>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let drag = view.device_drag.as_ref().unwrap();
            assert!(drag.lifted);
            drag.target.clone()
        })
    };
    assert_eq!(target(&view, cx), None);

    // Past the last header's middle, it lands after that device; the header
    // follows the pointer from where it was held, and the others close up.
    let below = point(header.center().x, c.bottom() - px(2.));
    for _ in 0..2 {
        cx.simulate_mouse_move(below, MouseButton::Left, Modifiers::default());
        draw(cx);
    }
    assert_eq!(target(&view, cx), Some((4, Beside::After("ssh:c".into()))));
    let carried = cx.debug_bounds("host-ssh:a").unwrap();
    assert_eq!(carried.top(), below.y - (header.center().y - header.top()));
    assert_eq!(cx.debug_bounds("host-ssh:b").unwrap().top(), rest);

    // The release is the drop, not a click selecting the device under it.
    cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.read_with(cx, |view, _| {
        assert!(view.device_drag.is_none());
        assert_eq!(view.selected_endpoint, 0);
        assert_eq!(view.device_order, ["local", "ssh:b", "ssh:c", "ssh:a"]);
    });
    assert_eq!(ids(&view, cx), ["local", "ssh:b", "ssh:c", "ssh:a"]);
    assert!(cx.debug_bounds("workspace-ssh:a-w0").is_some());

    // Escape puts a lifted header back without moving anything. The keys go
    // to the window's focus, as they do once anything has been clicked.
    view.update_in(cx, |view, window, cx| window.focus(&view.focus, cx));
    let header = cx.debug_bounds("host-ssh:c").unwrap();
    cx.simulate_mouse_down(header.center(), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(
        point(header.center().x, px(0.)),
        MouseButton::Left,
        Modifiers::default(),
    );
    draw(cx);
    assert!(cx.debug_bounds("workspace-ssh:c-w0").is_none());
    cx.simulate_keystrokes("escape");
    draw(cx);
    view.read_with(cx, |view, _| assert!(view.device_drag.is_none()));
    assert_eq!(ids(&view, cx), ["local", "ssh:b", "ssh:c", "ssh:a"]);
    assert!(cx.debug_bounds("workspace-ssh:c-w0").is_some());
}

/// Local and one device: either header lifts, and dropping Local below the
/// device puts it second.
#[gpui::test]
fn local_and_one_device_swap(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.live.snapshot = Some(Arc::new(snapshot(3)));
        view.endpoints.push(remote("a"));
        view
    });
    cx.simulate_resize(size(px(800.), px(900.)));
    draw(cx);
    let local = cx.debug_bounds("host-local").unwrap();
    cx.simulate_mouse_down(local.center(), MouseButton::Left, Modifiers::default());
    cx.executor().advance_clock(LIFT_DELAY);
    cx.run_until_parked();
    draw(cx);
    draw(cx);
    view.read_with(cx, |view, _| {
        assert!(view.device_drag.as_ref().unwrap().carries("local"))
    });
    // Folded, the device's header sits right under Local's place.
    let a = cx.debug_bounds("host-ssh:a").unwrap();
    assert!(cx.debug_bounds("workspace-local-w0").is_none());
    let below = point(local.center().x, a.bottom() + px(2.));
    for _ in 0..2 {
        cx.simulate_mouse_move(below, MouseButton::Left, Modifiers::default());
        draw(cx);
    }
    cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::default());
    draw(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.device_order, ["ssh:a", "local"]);
        assert_eq!(view.selected_endpoint, 0);
    });
    // The device's header is now above Local's.
    let a = cx.debug_bounds("host-ssh:a").unwrap();
    let local = cx.debug_bounds("host-local").unwrap();
    assert!(a.bottom() <= local.top());
}
