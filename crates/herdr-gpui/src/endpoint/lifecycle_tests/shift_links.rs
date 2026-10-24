//! A Shift-click on an address in an application that reads the mouse opens
//! it, as kitty does: Shift already keeps the press from the application.

use super::*;

#[gpui::test]
fn a_shift_click_opens_an_address_past_the_application(cx: &mut gpui::TestAppContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        Fixture(cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)))
    });
    let view = fixture.update(cx, |fixture, _| fixture.0.clone());
    let (endpoint, _server) = connected_endpoint("local:mouse");
    let address = "https://example.com/docs";
    let position = cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            prepare_mouse(view, endpoint, cx);
            let frame = &mut Arc::make_mut(view.live.surface.as_mut().unwrap()).frame;
            frame.cells = (0..frame.height)
                .flat_map(|y| (0..frame.width).map(move |x| (x, y)))
                .map(|(x, y)| CellData {
                    symbol: (y == 4)
                        .then(|| address.chars().nth(usize::from(x).wrapping_sub(1)))
                        .flatten()
                        .map_or_else(|| " ".into(), String::from),
                    fg: 0,
                    bg: 0,
                    modifier: 0,
                    skip: false,
                    hyperlink: None,
                })
                .collect();
            mouse_position(view, 5.5, 4.5)
        })
    });
    let shift = gpui::Modifiers::shift();
    let down = MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers: shift,
        click_count: 1,
        ..Default::default()
    };
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            // A plain click, or Shift with another modifier, is no link press.
            assert!(
                view.terminal_link_press(position, gpui::Modifiers::default())
                    .is_none()
            );
            let shift_alt = gpui::Modifiers { alt: true, ..shift };
            assert!(view.terminal_link_press(position, shift_alt).is_none());
            // Shift keeps the press here rather than sending it to the
            // application, and it is the address's.
            assert!(!view.terminal_mouse_down(&down, window, cx));
            let pressed = view.terminal_link_press(position, shift).unwrap();
            assert!(view.activate_terminal_link(&pressed, position, shift, false, window, cx));
        });
    });
    cx.run_until_parked();
    assert_eq!(cx.opened_url().as_deref(), Some(address));
}
