//! A stand-in for `ssh` that fails exactly as OpenSSH does when the `-F`
//! config it was handed no longer exists, and records what it found. It lets
//! the setup and reconnect flows run end to end without a network.
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

/// Route this thread's `ssh` commands to the stand-in until it drops. Every
/// run ends like a refused key (exit 255) and notes whether its config could
/// be read, which is the only thing it checks.
pub(crate) fn install() -> StandIn {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("runs");
    let script = dir.path().join("ssh.sh");
    std::fs::write(
        &script,
        format!(
            r#"config=
while [ $# -gt 0 ]; do
    case "$1" in -F) config=$2; shift 2;; *) shift;; esac
done
if [ -r "$config" ]; then
    echo readable >> '{log}'
    echo 'Permission denied (publickey).' >&2
else
    echo missing >> '{log}'
    echo "command-line: line 0: Can't open user config file $config: No such file or directory" >&2
fi
exit 255
"#,
            log = log.display()
        ),
    )
    .unwrap();
    SCRIPT.with(|slot| *slot.borrow_mut() = Some(script));
    StandIn { dir }
}

impl StandIn {
    /// One entry per `ssh` run: `readable` or `missing`.
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
