//! The Windows taskbar button: an overlay badge while agents need attention.
//!
//! Every top-level window has its own taskbar button, and windows sharing the
//! AppUserModelID are grouped into one. The grouped button shows the overlay
//! of whichever window last set one, so the badge is applied to every window
//! alike. The taskbar copies an overlay icon, so each is destroyed right after
//! it is set.
//!
//! All of this runs on the UI thread, where GPUI has initialized COM (OLE) as
//! a single-threaded apartment, so the interface is created and used there
//! and kept in a thread-local rather than a global that would need `Send`.

mod badge;
#[cfg(test)]
mod tests;

use gpui::{App, Window, WindowId};
use std::{cell::RefCell, collections::HashMap, time::Duration};
use windows::Win32::{
    Foundation::HWND,
    System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    UI::Shell::{ITaskbarList3, TaskbarList},
};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Explorer creates a window's taskbar button shortly after the window is
/// shown, and drops an overlay set before then. A new window gets its badge
/// again after this long.
const BUTTON_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("could not reach the taskbar")]
    Taskbar(#[source] windows::core::Error),
    #[error("could not draw the taskbar badge")]
    Badge(#[source] windows::core::Error),
    #[error("could not set the taskbar badge")]
    Overlay(#[source] windows::core::Error),
}

#[derive(Default)]
struct State {
    /// `None` until first needed; `Some(None)` once creating it failed, so a
    /// broken shell is not asked again on every change.
    taskbar: Option<Option<ITaskbarList3>>,
    windows: HashMap<WindowId, isize>,
    overlay: usize,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

impl State {
    fn taskbar(&mut self) -> Option<&ITaskbarList3> {
        self.taskbar
            .get_or_insert_with(|| {
                create()
                    .inspect_err(|error| tracing::warn!(%error, "taskbar integration is off"))
                    .ok()
            })
            .as_ref()
    }

    fn apply_overlay(&mut self, hwnd: isize) {
        let count = self.overlay;
        let Some(taskbar) = self.taskbar() else {
            return;
        };
        if let Err(error) = badge::set(taskbar, HWND(hwnd as *mut _), count) {
            tracing::debug!(%error, "taskbar badge was not set");
        }
    }
}

// The taskbar is reached only through COM, which has no safe wrapper.
#[allow(unsafe_code)]
fn create() -> Result<ITaskbarList3, Error> {
    // SAFETY: COM is initialized on this (the UI) thread by GPUI's Windows
    // platform before any window exists, and the class ID and interface are
    // the documented pair for the taskbar list.
    let taskbar: ITaskbarList3 =
        unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_INPROC_SERVER) }
            .map_err(Error::Taskbar)?;
    // SAFETY: `HrInit` must be called once before the other methods.
    unsafe { taskbar.HrInit() }.map_err(Error::Taskbar)?;
    Ok(taskbar)
}

/// Notes a window's native handle the first time it is seen, and gives it the
/// current badge. Cheap enough for the poll loop: a known window is one map
/// lookup.
pub(crate) fn track(window: &Window, cx: &mut App) {
    let id = window.window_handle().window_id();
    if STATE.with_borrow(|state| state.windows.contains_key(&id)) {
        return;
    }
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get();
    STATE.with_borrow_mut(|state| {
        state.windows.insert(id, hwnd);
        if state.overlay > 0 {
            state.apply_overlay(hwnd);
        }
    });
    cx.spawn(async move |cx| {
        cx.background_executor().timer(BUTTON_DELAY).await;
        STATE.with_borrow_mut(|state| {
            if state.overlay > 0 && state.windows.get(&id) == Some(&hwnd) {
                state.apply_overlay(hwnd);
            }
        });
    })
    .detach();
}

/// Drops windows that closed, so a reused handle is never badged by mistake.
pub(crate) fn retain(open: &[gpui::AnyWindowHandle]) {
    STATE.with_borrow_mut(|state| {
        state
            .windows
            .retain(|id, _| open.iter().any(|window| window.window_id() == *id));
    });
}

/// Shows `count` on every window's taskbar button, or clears it at zero.
/// Headless tests never reach the shell (see `app_badge::set_native`).
#[cfg(not(test))]
pub(crate) fn set_overlay(count: usize) {
    STATE.with_borrow_mut(|state| {
        state.overlay = count;
        let windows: Vec<isize> = state.windows.values().copied().collect();
        for hwnd in windows {
            state.apply_overlay(hwnd);
        }
    });
}
