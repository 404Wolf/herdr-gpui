//! How the Windows executables meet a console.
//!
//! Like Zed, the app is a GUI-subsystem program in release builds, so opening
//! it from Explorer or the Start menu never shows a console window, and a
//! console-subsystem copy of the same executable sits in `bin\` for terminals
//! and agents, which need a program the shell waits for and whose exit code it
//! sees. The packaging step makes that copy (`package-windows.py`); this
//! module is the runtime half:
//!
//! - The GUI build has no console. A command run through it anyway attaches
//!   to its caller's console so its output still shows, though the shell will
//!   not wait for it.
//! - The console copy, asked to open the app, starts the GUI build beside it
//!   and returns, so a terminal is not held open while the app runs.
//! - Agents are pointed at the console copy (see `agent_skill::executable`).
//!
//! Debug builds keep the console subsystem, as Zed's do, so `cargo run`
//! output stays in the terminal.

use std::path::{Path, PathBuf};

/// Attaches to the calling process's console when this process has none and
/// its output goes nowhere: the GUI build started from a terminal. Output
/// redirected to a file or pipe is left as it is.
#[cfg(windows)]
pub(crate) fn attach_parent() {
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
    };
    // SAFETY: GetStdHandle takes a standard-device constant and only reads
    // this process's parameters; a null result means no handle is set.
    #[allow(unsafe_code)]
    let unset = unsafe {
        GetStdHandle(STD_OUTPUT_HANDLE).is_null() && GetStdHandle(STD_ERROR_HANDLE).is_null()
    };
    if !unset {
        return;
    }
    // SAFETY: takes a process id constant; failure (no parent console, or one
    // already attached) leaves the process as it was and is ignored.
    #[allow(unsafe_code)]
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
pub(crate) fn attach_parent() {}

/// When this is the console copy in an installation's `bin\`, starts the app
/// beside it with `args` and returns the exit code to end with. `None` means
/// this process should open the window itself.
#[cfg(windows)]
pub(crate) fn open_app_beside(args: &[std::ffi::OsString]) -> Option<std::process::ExitCode> {
    let app = std::env::current_exe()
        .ok()
        .and_then(|cli| app_beside(&cli))
        .filter(|app| app.is_file())?;
    // The GUI build opens no console, so it needs no flags; it must not keep
    // this terminal's handles, or the shell would wait on them.
    let started = herdr_client::new_command(&app)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    Some(match started {
        Ok(_) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Unable to start {}: {error}", app.display());
            std::process::ExitCode::FAILURE
        }
    })
}

#[cfg(not(windows))]
pub(crate) fn open_app_beside(_: &[std::ffi::OsString]) -> Option<std::process::ExitCode> {
    None
}

/// The app beside the console copy `cli`, which must be in a `bin` directory.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn app_beside(cli: &Path) -> Option<PathBuf> {
    let bin = cli.parent()?;
    if !bin.file_name()?.eq_ignore_ascii_case("bin") {
        return None;
    }
    Some(bin.parent()?.join(cli.file_name()?))
}

/// The console copy for the app `app`, whether or not it exists.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) fn cli_beside(app: &Path) -> Option<PathBuf> {
    Some(app.parent()?.join("bin").join(app.file_name()?))
}

#[cfg(test)]
mod tests;
