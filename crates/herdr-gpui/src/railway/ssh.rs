//! The `ssh` command that reaches a Railway free VM: `ssh railway.new` with
//! one explicit identity. It runs from a private temporary config that also
//! replaces the user's own, so no alias, proxy, forward, or agent of theirs
//! applies, and only the chosen key is offered. Setup and every reconnect
//! build it the same way; the config lives until the caller drops the
//! [`Config`] and `ssh` reads it at start.
//!
//! Railway documents no host key for `railway.new`, so the first one seen is
//! pinned in a GUI-owned `known_hosts` and a later change is refused.
//! Railway prints a one-line JSON manifest on stderr for headless
//! connections; `manifest` reads its deadline during setup and nothing else.
//! Its links are never stored, logged, or opened.

use super::{Error, Result};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

const ALIAS: &str = "herdr-railway";
const HOST: &str = "railway.new";

/// A path inside a quoted `ssh_config` value: `%` starts a token there, and
/// nothing may end the line or the quotes.
fn quoted(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or(Error::KeyPath)?;
    if !path.is_absolute() || text.contains(['"', '\n', '\r', '\0']) {
        return Err(Error::KeyPath);
    }
    Ok(format!("\"{}\"", text.replace('%', "%%")))
}

pub(super) fn config_text(identity: &Path, known_hosts: &Path) -> Result<String> {
    Ok(format!(
        "Host {ALIAS}\n\
         \tHostName {HOST}\n\
         \tPort 22\n\
         \tAddressFamily inet\n\
         \tBatchMode yes\n\
         \tNumberOfPasswordPrompts 0\n\
         \tPreferredAuthentications publickey\n\
         \tPasswordAuthentication no\n\
         \tIdentitiesOnly yes\n\
         \tIdentityFile {identity}\n\
         \tStrictHostKeyChecking accept-new\n\
         \tVerifyHostKeyDNS no\n\
         \tUserKnownHostsFile {known_hosts}\n\
         \tGlobalKnownHostsFile /dev/null\n\
         \tConnectTimeout 15\n\
         \tConnectionAttempts 1\n\
         \tServerAliveInterval 15\n\
         \tServerAliveCountMax 4\n\
         \tForwardAgent no\n\
         \tForwardX11 no\n\
         \tClearAllForwardings yes\n\
         \tControlMaster no\n\
         \tControlPath none\n\
         \tProxyCommand none\n\
         \tPermitLocalCommand no\n\
         \tLogLevel ERROR\n",
        identity = quoted(identity)?,
        known_hosts = quoted(known_hosts)?,
    ))
}

/// The private temporary config an `ssh` command reads. Keep it alive until
/// every `ssh` run built from it has started (`ssh` reads it at start); it is
/// removed on drop. Building each command from one `Config` held by the caller
/// is what keeps the file from vanishing between building and spawning.
pub(crate) struct Config {
    file: tempfile::NamedTempFile,
}

impl Config {
    pub(crate) fn new(identity: &Path, known_hosts: &Path) -> Result<Self> {
        let text = config_text(identity, known_hosts)?;
        if let Some(parent) = known_hosts.parent() {
            std::fs::create_dir_all(parent).map_err(Error::ConfigFile)?;
        }
        // Created 0600 in the temporary directory; removed on drop.
        let mut file = tempfile::NamedTempFile::new().map_err(Error::ConfigFile)?;
        file.write_all(text.as_bytes())
            .and_then(|()| file.flush())
            .map_err(Error::ConfigFile)?;
        Ok(Self { file })
    }

    /// A fresh `ssh` command reading this config; append the remote command.
    pub(crate) fn command(&self) -> Command {
        let mut command = base();
        command
            .arg("-F")
            .arg(self.file.path())
            .args(["-T", "--", ALIAS]);
        command
    }
}

/// The command every `ssh` run starts from. Tests on one thread may stand in
/// a script that checks what `ssh` would be handed, such as a config file that
/// must still exist when the process starts.
fn base() -> Command {
    #[cfg(test)]
    if let Some(command) = stand_in::base() {
        return command;
    }
    Command::new("ssh")
}

/// A stand-in for `ssh` that reports, like OpenSSH, a config file that is gone.
#[cfg(all(test, unix))]
pub(super) mod stand_in;
#[cfg(all(test, not(unix)))]
mod stand_in {
    pub(super) fn base() -> Option<std::process::Command> {
        None
    }
}

/// The GUI's own record of the host key `railway.new` presented.
pub(crate) fn known_hosts(dir: &Path) -> PathBuf {
    dir.join("railway_known_hosts")
}

// Tests drive `ssh`, `ssh-keygen`, and `/bin/sh`; cloud devices are Unix-only.
#[cfg(all(test, unix))]
mod tests;
