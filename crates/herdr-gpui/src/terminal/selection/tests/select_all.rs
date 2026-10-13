use super::*;

fn all(s: &PaneSurfaceFrame, target: InputTarget) -> Option<Selection> {
    Selection::all(s, target, CELL_WIDTH, CELL_HEIGHT)
}

fn pane() -> InputTarget {
    InputTarget::Pane("pane".into())
}

fn whole(rows: u32) -> (String, TextRange) {
    (
        "pane".to_owned(),
        TextRange {
            start: TextPoint { row: 0, col: 0 },
            end: TextPoint {
                row: rows - 1,
                col: 9,
            },
        },
    )
}

#[test]
fn select_all_takes_a_pane_s_whole_screen_released_and_copies_it_from_the_cells() {
    let s = surface("abcdefghijklmnopqrstuvw");
    let selection = all(&s, pane()).unwrap();
    assert!(!selection.dragging() && selection.in_panes());
    assert_eq!(
        selection
            .rows(&s, CELL_WIDTH, CELL_HEIGHT)
            .collect::<Vec<_>>(),
        vec![(0, 0..10), (1, 0..10), (2, 0..10)]
    );
    assert_eq!(text(&s, &selection), "abcdefghij\nklmnopqrst\nuvw");
    assert!(
        selection
            .offscreen_range(&s, CELL_WIDTH, CELL_HEIGHT)
            .is_none()
    );
}

/// History above the screen and rows below a pane scrolled up into it are
/// both part of it, read back from the daemon rather than the painted cells.
#[test]
fn select_all_reaches_the_history_and_the_rows_below_a_scrolled_pane() {
    for top in [10, 4, 0] {
        let s = scrolled("abcdefghij", top);
        let selection = all(&s, pane()).unwrap();
        let expected = Some(whole(13));
        let range =
            |range: Option<(&str, TextRange)>| range.map(|(pane, range)| (pane.to_owned(), range));
        assert_eq!(
            range(selection.content_range(&s, CELL_WIDTH, CELL_HEIGHT)),
            expected,
            "scrolled to row {top}"
        );
        assert_eq!(
            range(selection.offscreen_range(&s, CELL_WIDTH, CELL_HEIGHT)),
            expected,
            "scrolled to row {top}"
        );
        assert!(matches!(
            selection.text(&s, CELL_WIDTH, CELL_HEIGHT),
            Err(Error::SelectionOffscreen)
        ));
    }
}

#[test]
fn select_all_takes_a_popup_s_whole_grid() {
    let mut s = surface("abcdefghij");
    s.popup = Some(Box::new(ClientShellPopupSurface {
        terminal_id: "popup".into(),
        title: String::new(),
        width: None,
        height: None,
        frame: frame("popup", 4, 2),
        mouse_reporting: false,
        sgr_pixel_mouse: false,
        pixel_width: 40,
        pixel_height: 40,
    }));
    let selection = all(&s, InputTarget::Popup("popup".into())).unwrap();
    assert!(selection.in_popup("popup"));
    assert_eq!(text(&s, &selection), "popu\np");
    // The panes under the popup are not on screen to select.
    assert!(all(&s, pane()).is_none());
    assert!(all(&s, InputTarget::Popup("other".into())).is_none());
}

#[test]
fn select_all_needs_a_pane_the_surface_paints() {
    let s = surface("abcdefghij");
    assert!(all(&s, InputTarget::Pane("gone".into())).is_none());
    let mut empty = s.clone();
    empty.panes[0].inner_rect.width = 0;
    assert!(all(&empty, pane()).is_none());
}

/// A pane that keeps printing, as an agent redrawing its status does, keeps
/// the highlight until the copy that follows it.
#[test]
fn select_all_survives_output_written_over_its_rows() {
    let s = surface("abcdefghij");
    let selection = all(&s, pane()).unwrap();
    let redrawn = surface("zzzzzzzzzz");
    assert!(selection.text_unchanged(&redrawn, CELL_WIDTH, CELL_HEIGHT));
}
