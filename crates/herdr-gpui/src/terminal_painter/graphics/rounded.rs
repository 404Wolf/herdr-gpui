//! Rounded box corners meet the same device-pixel stroke bands as straight
//! lines. Font glyphs cannot cover configurable padding between terminal rows.
use super::*;

impl Graphic {
    pub(in crate::terminal_painter) fn paint(
        self,
        cell: Bounds<Pixels>,
        color: Rgba,
        window: &mut Window,
    ) -> gpui::Result<usize> {
        let cell = Bounds::from_corners(
            window.pixel_snap_point(cell.origin),
            window.pixel_snap_point(cell.bottom_right()),
        );
        if let Self::Rounded { right, down } = self {
            let geometry = geometry(cell, window.scale_factor(), right, down);
            let mut path = PathBuilder::stroke(geometry.stroke);
            path.move_to(geometry.start);
            path.line_to(geometry.horizontal);
            path.curve_to(geometry.vertical, geometry.control);
            path.line_to(geometry.end);
            window.paint_path(path.build()?, color);
            return Ok(0);
        }
        let mut count = 0;
        self.rectangles(cell, window.scale_factor(), |bounds| {
            window.paint_quad(fill(bounds, color));
            count += 1;
        });
        Ok(count)
    }
}

struct Geometry {
    start: Point<Pixels>,
    horizontal: Point<Pixels>,
    control: Point<Pixels>,
    vertical: Point<Pixels>,
    end: Point<Pixels>,
    stroke: Pixels,
}

fn geometry(cell: Bounds<Pixels>, scale: f32, right: bool, down: bool) -> Geometry {
    let left = f32::from(cell.left()) * scale;
    let top = f32::from(cell.top()) * scale;
    let width = f32::from(cell.size.width) * scale;
    let height = f32::from(cell.size.height) * scale;
    let stroke = (width / 8.).round().max(1.);
    let x = (left + width / 2. - stroke / 2.).floor() + stroke / 2.;
    let y = (top + height / 2. - stroke / 2.).floor() + stroke / 2.;
    let radius = (width.min(height) / 2. - stroke).max(stroke);
    let at = |x, y| point(px(x / scale), px(y / scale));
    Geometry {
        start: at(if right { left + width } else { left }, y),
        horizontal: at(x + if right { radius } else { -radius }, y),
        control: at(x, y),
        vertical: at(x, y + if down { radius } else { -radius }),
        end: at(x, if down { top + height } else { top }),
        stroke: px(stroke / scale),
    }
}

#[cfg(test)]
mod tests;
