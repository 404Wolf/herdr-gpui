use super::*;
use crate::config::{Backdrop, backdrop::Edit};
use crate::settings_window::controls::backdrop::{Mode, stepped};

fn backdrop(opacity: f32, image: Option<&str>) -> Backdrop {
    Backdrop {
        opacity,
        blur: false,
        image: image.map(Into::into),
    }
}

#[test]
fn the_mode_follows_the_config_and_switching_writes_only_what_differs() {
    let solid = backdrop(1., None);
    let translucent = backdrop(0.7, None);
    let picture = backdrop(0.7, Some("/pictures/sea.jpg"));
    assert_eq!(Mode::of(&solid), Mode::Solid);
    assert_eq!(Mode::of(&translucent), Mode::Translucent);
    assert_eq!(Mode::of(&picture), Mode::Picture);
    // A picture under a solid background is still the picture mode.
    assert_eq!(Mode::of(&backdrop(1., Some("/a.png"))), Mode::Picture);

    assert_eq!(Mode::Translucent.edits(&solid), [Edit::Opacity(0.85)]);
    // Leaving a picture keeps the opacity it was shown at.
    assert_eq!(Mode::Translucent.edits(&picture), [Edit::Image(None)]);
    assert_eq!(Mode::Solid.edits(&translucent), [Edit::Opacity(1.)]);
    assert_eq!(
        Mode::Solid.edits(&picture),
        [Edit::Image(None), Edit::Opacity(1.)]
    );
    // Picture needs a file, which only the chooser gives.
    assert!(Mode::Picture.edits(&solid).is_empty());
}

#[test]
fn the_stepper_moves_on_a_five_percent_grid_within_its_range() {
    assert_eq!(stepped(0.85, false), Some(0.8));
    assert_eq!(stepped(0.85, true), Some(0.9));
    // A value from the file lands on the grid.
    assert_eq!(stepped(0.83, true), Some(0.9));
    assert_eq!(stepped(0.95, true), Some(1.));
    assert_eq!(stepped(1., true), None);
    assert_eq!(stepped(0.1, false), None);
    // Below the floor, stepping up lands on it.
    assert_eq!(stepped(0., true), Some(0.1));
}

#[gpui::test]
fn each_mode_shows_three_tiles_and_only_its_own_controls(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(skill_fixture);
    cx.simulate_resize(size(px(960.), px(3200.)));
    for (config, stepper, blur, picture) in [
        (backdrop(1., None), false, false, false),
        (backdrop(0.8, None), true, true, false),
        (backdrop(0.75, Some("/pictures/sea.jpg")), true, false, true),
    ] {
        view.update(cx, |view, cx| {
            view.section = Section::Appearance;
            view.config.backdrop = config;
            cx.notify();
        });
        cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
        for tile in [
            "settings-backdrop-solid",
            "settings-backdrop-translucent",
            "settings-backdrop-picture",
        ] {
            assert!(cx.debug_bounds(tile).is_some(), "{tile}");
        }
        assert_eq!(
            cx.debug_bounds("settings-backdrop-opacity").is_some(),
            stepper
        );
        assert_eq!(cx.debug_bounds("settings-backdrop-blur").is_some(), blur);
        assert_eq!(
            cx.debug_bounds("settings-backdrop-remove").is_some(),
            picture
        );
        // The tiles share one row and stay inside the card.
        let card = cx.debug_bounds("settings-backdrop").unwrap();
        let solid = cx.debug_bounds("settings-backdrop-solid").unwrap();
        let tile = cx.debug_bounds("settings-backdrop-picture").unwrap();
        assert_eq!(solid.top(), tile.top());
        assert!(tile.right() <= card.right());
    }
}
