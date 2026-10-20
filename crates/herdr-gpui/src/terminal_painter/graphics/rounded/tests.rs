use super::*;
use core::prelude::v1::test;
use gpui::size;

#[test]
fn all_four_corners_join_straight_strokes_at_every_line_height() {
    for scale in [1., 1.5, 2.] {
        for height in [14., 20., 28.] {
            let cell = Bounds::new(point(px(10.), px(20.)), size(px(10.), px(height)));
            for (symbol, right, down) in [
                ("╭", true, true),
                ("╮", false, true),
                ("╰", true, false),
                ("╯", false, false),
            ] {
                assert!(matches!(
                    Graphic::from_symbol(symbol),
                    Some(Graphic::Rounded { .. })
                ));
                let g = geometry(cell, scale, right, down);
                assert_eq!(g.start.x, if right { cell.right() } else { cell.left() });
                assert_eq!(g.end.y, if down { cell.bottom() } else { cell.top() });
                let mut vertical = Vec::new();
                Graphic::Lines([0, 0, 1, 1]).rectangles(cell, scale, |rect| vertical.push(rect));
                for rect in vertical {
                    let device = |p: Pixels| (f32::from(p) * scale).round();
                    assert_eq!(device(g.end.x - g.stroke / 2.), device(rect.left()));
                    assert_eq!(device(g.end.x + g.stroke / 2.), device(rect.right()));
                }
                let mut horizontal = Vec::new();
                Graphic::Lines([1, 1, 0, 0]).rectangles(cell, scale, |rect| horizontal.push(rect));
                for rect in horizontal {
                    assert!((f32::from(g.start.y - g.stroke / 2. - rect.top())).abs() < 0.001);
                    assert!((f32::from(g.start.y + g.stroke / 2. - rect.bottom())).abs() < 0.001);
                }
            }
        }
    }
}
