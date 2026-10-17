//! Windows 11 system backdrops. GPUI sets Mica through DWM but blurs with the
//! older `SetWindowCompositionAttribute` acrylic, which lags behind a moving
//! or resizing window. Where DWM has system backdrops, a blurred window asks
//! it for acrylic instead, the material Windows draws its own flyouts with.
//!
//! The two mechanisms do not reset each other: GPUI clears the composition
//! accent only when switching to another accent, and never clears the DWM
//! backdrop. Each switch here undoes whichever of them the previous
//! appearance left behind.
use crate::caption::windows::{
    DWMSBT_AUTO, DWMSBT_TRANSIENTWINDOW, system_backdrop, system_backdrops,
};
use gpui::{Window, WindowBackgroundAppearance as Appearance};

/// How an appearance reaches the window on this Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    /// GPUI's composition accent: transparent, or acrylic before 22H2.
    Accent,
    /// A DWM system backdrop: Mica through GPUI, acrylic here.
    Dwm,
    /// Neither: the opaque window.
    None,
}

fn route(appearance: Appearance, modern: bool) -> Route {
    match appearance {
        Appearance::Opaque => Route::None,
        Appearance::Blurred if modern => Route::Dwm,
        Appearance::Transparent | Appearance::Blurred => Route::Accent,
        // Mica on an older Windows is a no-op in GPUI, as here.
        _ => Route::Dwm,
    }
}

pub(super) fn apply(window: &mut Window, previous: Appearance, next: Appearance) {
    let modern = system_backdrops();
    let (from, to) = (route(previous, modern), route(next, modern));
    if from == Route::Accent && to == Route::Dwm {
        window.set_background_appearance(Appearance::Opaque);
    }
    if from == Route::Dwm && to != Route::Dwm {
        system_backdrop(window, DWMSBT_AUTO);
    }
    if next == Appearance::Blurred && to == Route::Dwm {
        // GPUI's Mica leaves the window transparent with no accent; the
        // backdrop it asked for is then replaced with acrylic.
        window.set_background_appearance(Appearance::MicaBackdrop);
        system_backdrop(window, DWMSBT_TRANSIENTWINDOW);
    } else {
        window.set_background_appearance(next);
    }
}

#[cfg(test)]
mod tests;
