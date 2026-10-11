//! A stand-in for `ssh` that lets the setup and reconnect flows run end to end
//! without a network. [`install`] fails exactly as OpenSSH does when the `-F`
//! config it was handed no longer exists; [`install_script`] runs any script.
#![allow(clippy::unwrap_used)]

use std::{cell::RefCell, path::PathBuf, process::Command};

thread_local! {
    static SCRIPT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// `sh <script>` while a [`StandIn`] lives on this thread, so no freshly
/// written file is ever executed directly.
pub(super) fn base() -> Option<Command> {
    SCRIPT.with(|script| {
        script.borrow().as_ref().map(|script| {
            let mut command = Command::new("/bin/sh");
            command.arg(script);
            command
        })
    })
}

pub(crate) struct StandIn {
    dir: tempfile::TempDir,
}

/// Route this thread's `ssh` commands to a stand-in that ends like a refused
/// key (exit 255) and notes whether its config could be read.
pub(crate) fn install() -> StandIn {
    install_script(
        r#"config=
while [ $# -gt 0 ]; do
    case "$1" in -F) config=$2; shift 2;; *) shift;; esac
done
if [ -r "$config" ]; then
    echo readable >> '@LOG@'
    echo 'Permission denied (publickey).' >&2
else
    echo missing >> '@LOG@'
    echo "command-line: line 0: Can't open user config file $config: No such file or directory" >&2
fi
exit 255
"#,
    )
}

/// Route this thread's `ssh` commands to `sh` running `body`, in which `@LOG@`
/// is a file each run may append lines to (see [`StandIn::runs`]) and `@STATE@`
/// a directory that outlives the runs, for a fake service's memory.
pub(crate) fn install_script(body: &str) -> StandIn {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("runs");
    let state = dir.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let script = dir.path().join("ssh.sh");
    std::fs::write(
        &script,
        body.replace("@LOG@", &log.display().to_string())
            .replace("@STATE@", &state.display().to_string()),
    )
    .unwrap();
    SCRIPT.with(|slot| *slot.borrow_mut() = Some(script));
    StandIn { dir }
}

impl StandIn {
    /// The lines the runs appended to their log.
    pub(crate) fn runs(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("runs"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        SCRIPT.with(|slot| *slot.borrow_mut() = None);
    }
}
