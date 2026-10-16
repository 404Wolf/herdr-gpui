//! The Windows console the GUI was handed at launch.
//!
//! `herdr-gpui.exe` is a console-subsystem program so `herdr-gpui COMMAND`
//! prints to, and returns its exit code to, the shell that ran it. The cost is
//! that Windows gives the GUI its own console window when it starts from
//! Explorer, a shortcut, or the Run dialog. A GUI launch hides that window when
//! no other process shares the console, so it was created for this app alone.
//!
//! The console is hidden rather than released: the app's console children
//! (git, ssh, shells) inherit it, and without one each would open a window of
//! its own. A console shared with the shell that started the app stays as is.

#[cfg(windows)]
pub(crate) fn hide_owned_window() {
    use windows_sys::Win32::{
        System::Console::{GetConsoleProcessList, GetConsoleWindow},
        UI::WindowsAndMessaging::{SW_HIDE, ShowWindow},
    };
    // Room for this process and one more is enough to tell "alone" apart.
    let mut processes = [0u32; 2];
    // SAFETY: the buffer is live and writable, and its length is passed exactly.
    #[allow(unsafe_code)]
    let attached = unsafe { GetConsoleProcessList(processes.as_mut_ptr(), 2) };
    if !owns_console(attached) {
        return;
    }
    // SAFETY: takes no arguments; returns null when there is no console window.
    #[allow(unsafe_code)]
    let window = unsafe { GetConsoleWindow() };
    if window.is_null() {
        return;
    }
    // SAFETY: `window` is this process's console window, checked non-null above;
    // ShowWindow only changes its visibility.
    #[allow(unsafe_code)]
    unsafe {
        ShowWindow(window, SW_HIDE);
    }
}

#[cfg(not(windows))]
pub(crate) fn hide_owned_window() {}

/// Whether a console with `attached` processes was created for this one alone.
/// Zero means the query failed or there is no console: leave it alone.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn owns_console(attached: u32) -> bool {
    attached == 1
}

#[cfg(test)]
mod tests;
