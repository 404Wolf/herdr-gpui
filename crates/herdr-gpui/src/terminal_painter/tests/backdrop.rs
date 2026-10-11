use super::*;

/// Paints one row — the default background, a program's color, and the
/// theme's background chosen explicitly — and returns the quad colors.
fn row_quads(ground: Ground, cx: &mut TestAppContext) -> Vec<Background> {
    let (_, cx) = cx.add_window_view(|_, _| Empty);
    let theme = Theme::default();
    cx.draw(Point::default(), size(px(100.), px(40.)), move |_, _| {
        canvas(
            |_, _, _| (),
            move |_, _, window, cx| {
                let frame = FrameData {
                    width: 3,
                    height: 1,
                    cells: [0, 0x02000000 | 0x123456, 0x02000000 | theme.background]
                        .into_iter()
                        .map(|bg| CellData { bg, ..cell("a") })
                        .collect(),
                    cursor: None,
                    hyperlinks: vec![],
                    graphics: vec![],
                };
                let mut painter = TerminalPainter::default();
                painter.set_appearance(14., 20., theme.clone());
                painter.paint_frame(
                    &frame,
                    point(px(0.), px(0.)),
                    Some(size(px(30.), px(20.))),
                    ground,
                    10.,
                    &font("Menlo"),
                    &[],
                    &[],
                    None,
                    None,
                    window,
                    cx,
                );
            },
        )
        .size_full()
    });
    cx.update(|window, _| {
        window
            .painted_quads()
            .iter()
            .map(|quad| quad.background)
            .collect()
    })
}

#[gpui::test]
fn a_backdrop_skips_only_cells_on_the_themes_background(cx: &mut TestAppContext) {
    let background: Background = rgb(Theme::default().background).into();
    let chosen: Background = rgb(0x123456).into();
    // Opaque, every cell paints, the theme's color on both sides of the
    // program's.
    assert_eq!(
        row_quads(Ground::Opaque, cx),
        [background, chosen, background]
    );
    // Over a backdrop the ground already is that color: painting it again
    // would stack its alpha, so only the program's color paints.
    assert_eq!(row_quads(Ground::Backdrop, cx), [chosen]);
}
