//! Registers the app with Windows Restart Manager, so an installer or Windows
//! Update that closes it to replace files, or a restart with **Restart apps**
//! (Settings > Accounts > Sign-in options) turned on, opens it again.
//!
//! The app reopens without arguments, as its Start menu entry starts it: a
//! session or socket chosen on the command line is not carried over. It is
//! never restarted after a crash or a hang, which could otherwise loop.

/// Call once from a normal GUI launch; fixtures and commands never register.
#[cfg(windows)]
pub(crate) fn register() {
    use windows_sys::Win32::System::Recovery::{
        RESTART_NO_CRASH, RESTART_NO_HANG, RegisterApplicationRestart,
    };
    // SAFETY: a null command line asks for a restart without arguments, and
    // the flags are constants; the call reads nothing else from this process.
    #[allow(unsafe_code)]
    let result =
        unsafe { RegisterApplicationRestart(std::ptr::null(), RESTART_NO_CRASH | RESTART_NO_HANG) };
    if result != 0 {
        tracing::warn!(hresult = result, "Restart Manager registration failed");
    }
}

#[cfg(not(windows))]
pub(crate) fn register() {}
