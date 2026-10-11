//! The desktop blur behind a translucent window on macOS, asked of the
//! window server directly.
//!
//! GPUI's `Blurred` appearance puts an `NSVisualEffectView` behind its
//! surface and strips that view's layers of their tint and saturation. On
//! macOS 26 and later that leaves the desktop unblurred: the window looks
//! the same as `Transparent`. The window server's own per-window blur, which
//! WezTerm, Ghostty, and kitty use, still works, so the window asks GPUI for
//! `Transparent` and sets the blur itself.
//!
//! The window server's call has no public or safe binding, and reaching the
//! `NSWindow` behind GPUI's handle means dereferencing the raw view pointer,
//! so both need `unsafe`.
#![allow(unsafe_code)]

use gpui::{Window, WindowBackgroundAppearance};
use std::ffi::c_int;
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// How far the desktop is blurred, in points.
const RADIUS: c_int = 24;

// Exported by CoreGraphics, from the window server's SkyLight framework. No
// public API blurs what lies behind a window, and the AppKit view that should
// is what fails above.
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGSDefaultConnectionForThread() -> c_int;
    fn CGSSetWindowBackgroundBlurRadius(connection: c_int, window: isize, radius: c_int) -> c_int;
}

/// Moves `window` from the appearance `was` to `appearance`, blurring
/// through the window server rather than GPUI.
pub(super) fn apply(
    window: &mut Window,
    was: WindowBackgroundAppearance,
    appearance: WindowBackgroundAppearance,
) {
    let blurred = appearance == WindowBackgroundAppearance::Blurred;
    window.set_background_appearance(if blurred {
        WindowBackgroundAppearance::Transparent
    } else {
        appearance
    });
    if blurred || was == WindowBackgroundAppearance::Blurred {
        set(window, blurred);
    }
}

/// Blurs the desktop behind `window`, or stops blurring it.
fn set(window: &Window, blur: bool) {
    let Some(number) = window_number(window) else {
        tracing::warn!("Could not find the native window to blur behind");
        return;
    };
    // SAFETY: both functions take plain integers and touch no memory of
    // ours. The connection is this thread's own, and `number` names a window
    // this process owns, as the window server checks.
    let status = unsafe {
        CGSSetWindowBackgroundBlurRadius(
            CGSDefaultConnectionForThread(),
            number,
            if blur { RADIUS } else { 0 },
        )
    };
    if status != 0 {
        tracing::warn!(status, "The window server refused the backdrop blur");
    }
}

/// The window server's number for `window`.
fn window_number(window: &Window) -> Option<isize> {
    // Window has an inherent `window_handle` of its own.
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // AppKit views may only be used on the main thread, where GPUI renders.
    objc2::MainThreadMarker::new()?;
    // SAFETY: the handle borrows `window`, whose GPUI view is alive and an
    // `NSView` for as long as that borrow, and the check above holds us to
    // the main thread.
    let view = unsafe { handle.ns_view.cast::<objc2_app_kit::NSView>().as_ref() };
    Some(view.window()?.windowNumber())
}
