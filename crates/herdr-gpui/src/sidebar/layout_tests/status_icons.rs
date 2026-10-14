use super::*;

/// Every layout draws the chosen icon set: the rail style's bar sits on each
/// working row's leading edge and nowhere else, and Herdr's own rows center
/// the larger icon on their text column instead of its first line.
#[gpui::test]
fn status_icons_fit_every_layout(cx: &mut gpui::TestAppContext) {
    use crate::config::{LayoutMode, StatusIcons};
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(800.), px(900.)));
    // Devices draws Herdr's rows under device headers, which this fixture has none of.
    let modes = LayoutMode::ALL
        .into_iter()
        .filter(|mode| *mode != LayoutMode::Devices);
    for (mode, icons, font) in modes.flat_map(|mode| {
        StatusIcons::ALL
            .into_iter()
            .flat_map(move |icons| [13., 20.].map(|font| (mode, icons, font)))
    }) {
        view.update(cx, |view, cx| {
            view.config.layout.mode = mode;
            view.config.status_icons = Some(icons);
            view.config.sidebar.size = font;
            cx.notify();
        });
        cx.run_until_parked();
        cx.update(|window, cx| full_draw(window, cx).clear(cx));
        let case = format!("{mode:?} {icons:?} at {font}");
        let sidebar = cx.debug_bounds("sidebar").unwrap();
        for key in ["herdr", "agent-p0"] {
            let row = cx
                .debug_bounds(Box::leak(format!("row-{key}").into_boxed_str()))
                .unwrap_or_else(|| panic!("{case}: no row-{key}"));
            assert!(row.right() <= sidebar.right(), "{case}: row-{key} {row:?}");
            let rail = cx.debug_bounds(Box::leak(format!("rail-{key}").into_boxed_str()));
            match (icons, rail) {
                (StatusIcons::Rail, Some(rail)) => assert!(
                    // Orca's cards are bordered, and the rail sits inside it.
                    rail.left() >= row.left()
                        && rail.left() - row.left() <= px(1.)
                        && rail.top() >= row.top()
                        && rail.bottom() <= row.bottom(),
                    "{case}: rail-{key} {rail:?} is not on row {row:?}"
                ),
                (StatusIcons::Rail, None) => panic!("{case}: working row-{key} has no rail"),
                (_, Some(rail)) => panic!("{case}: unexpected rail-{key} {rail:?}"),
                (_, None) => {}
            }
        }
        if let LayoutMode::Classic { .. } = mode {
            let row = cx.debug_bounds("row-herdr").unwrap();
            let mark = cx.debug_bounds("mark-herdr").unwrap();
            let column = cx.debug_bounds("column-herdr").unwrap();
            assert!(
                mark.right() <= column.left(),
                "{case}: {mark:?} overlaps {column:?}"
            );
            assert!(
                (mark.center().y - column.center().y).abs() <= px(1.),
                "{case}: {mark:?} is not centered on {column:?}"
            );
            assert!(
                mark.top() >= row.top() && mark.bottom() <= row.bottom(),
                "{case}: {mark:?} leaves {row:?}"
            );
        }
    }
}
