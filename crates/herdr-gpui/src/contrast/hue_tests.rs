//! The hue shift behind tints that must not cost text its contrast.
use super::*;

#[test]
fn a_hue_shift_turns_the_hue_and_keeps_the_lightness_where_it_fits() {
    // A dark surface has room for a deep blue at its own lightness.
    let base = 0x1c1c22;
    let shifted = hue_shift(base, 0x1867ff, 0.1);
    assert!((to_oklab(shifted)[0] - to_oklab(base)[0]).abs() < 0.01);
    assert!((hue_degrees(shifted) - hue_degrees(0x1867ff)).abs() < 8.);
    assert!(distance(shifted, base) > 0.08);
}

#[test]
fn near_white_it_steps_toward_grey_rather_than_bending_the_hue() {
    for base in [0xffffff, 0xeff1f5, 0xf6f8fa] {
        let shifted = hue_shift(base, 0x0969da, 0.15);
        let moved = to_oklab(base)[0] - to_oklab(shifted)[0];
        assert!(
            (0. ..=HUE_SHIFT_LIGHTNESS + 0.01).contains(&moved),
            "{base:06x}: {moved}"
        );
        assert!(
            (hue_degrees(shifted) - hue_degrees(0x0969da)).abs() < 10.,
            "{base:06x} -> {shifted:06x}"
        );
    }
}

#[test]
fn a_grey_has_no_hue_to_give() {
    assert_eq!(hue_shift(0x1c1c22, 0x808080, 0.15), 0x1c1c22);
}
