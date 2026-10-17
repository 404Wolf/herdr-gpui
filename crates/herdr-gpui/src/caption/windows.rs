//! DWM window attributes GPUI does not set from the app's own state.
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Graphics::Dwm::{
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWINDOWATTRIBUTE, DwmSetWindowAttribute,
};

/// Darkens or lightens the system caption of `window`.
pub(crate) fn dark_caption(window: &Window, dark: bool) {
    set(window, DWMWA_USE_IMMERSIVE_DARK_MODE, i32::from(dark));
}

/// Sets a 32-bit attribute such as a `BOOL`. A failure leaves the window as
/// GPUI drew it, which is still usable, so it is logged rather than surfaced.
fn set(window: &Window, attribute: DWMWINDOWATTRIBUTE, value: i32) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    // Safe Rust has no DWM binding; windows-sys is the plain FFI declaration.
    // SAFETY: the handle comes from a live GPUI window borrowed for this
    // call, so the HWND is valid; `value` outlives the call and its size is
    // passed with it. DwmSetWindowAttribute only reads the buffer.
    #[allow(unsafe_code)]
    let result = unsafe {
        DwmSetWindowAttribute(
            handle.hwnd.get() as _,
            attribute as u32,
            std::ptr::from_ref(&value).cast(),
            size_of::<i32>() as u32,
        )
    };
    if result < 0 {
        tracing::debug!(attribute, result, "DwmSetWindowAttribute failed");
    }
}
