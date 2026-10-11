//! SSH keys for Railway devices, through the system's `ssh-keygen`: reading a
//! key's fingerprint (which names its device) and generating a new one. The
//! GUI never parses or holds private key bytes. Runs are bounded, take no
//! terminal input, and observe cancellation.

use super::{Error, Result, identity};
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(30);
const OUTPUT_LIMIT: u64 = 4096;
const COMMENT: &str = "herdr-gpui railway";

/// Fail before anything is created when the tools cannot run: a VM made for a
/// device that can never connect still counts against Railway's daily limit.
pub(crate) fn check_tools() -> Result<()> {
    let run = |program: &str, argument: &str| {
        Command::new(program)
            .arg(argument)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(drop)
    };
    run("ssh", "-V").map_err(Error::SshMissing)?;
    // `ssh-keygen` has no version flag; an unknown one prints usage and fails
    // after starting, which is all this proves.
    run("ssh-keygen", "-?").map_err(Error::KeygenMissing)
}

fn run(mut command: Command, cancelled: &impl Fn() -> bool) -> Result<(Option<i32>, String)> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(Error::KeygenMissing)?;
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => return Err(Error::KeygenMissing(error)),
        }
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Cloud(crate::cloud::Error::Cancelled));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::KeygenTimeout);
        }
        thread::sleep(Duration::from_millis(20));
    };
    let mut output = String::new();
    if let Some(stdout) = child.stdout.take() {
        let _ = stdout.take(OUTPUT_LIMIT).read_to_string(&mut output);
    }
    Ok((status.code(), output))
}

/// A device ID from `ssh-keygen -l` output such as
/// `256 SHA256:abc… comment (ED25519)`.
pub(super) fn device_id(output: &str) -> Option<String> {
    let digest = output
        .lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .strip_prefix("SHA256:")?;
    let id: String = digest
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(16)
        .collect();
    (id.len() == 16).then(|| format!("key-{id}"))
}

/// The device ID for the key at `path`. An empty passphrase is supplied so a
/// protected key falls back to its `.pub` file instead of prompting.
pub(super) fn fingerprint(path: &Path, cancelled: &impl Fn() -> bool) -> Result<String> {
    let mut command = Command::new("ssh-keygen");
    command
        .args(["-l", "-E", "sha256", "-P", ""])
        .arg("-f")
        .arg(path);
    match run(command, cancelled)? {
        (Some(0), output) => device_id(&output).ok_or_else(|| Error::UnusableKey(path.to_owned())),
        _ => Err(Error::UnusableKey(path.to_owned())),
    }
}

/// A key chosen from the user's own files: an absolute path to a regular
/// file. Symlinks are followed by `ssh` itself, so the path is kept as given
/// rather than resolved, and nothing is read here but the fingerprint.
pub(super) fn existing(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(Error::KeyPath);
    }
    let metadata = fs::metadata(path).map_err(|source| Error::KeyFile {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(Error::KeyPath);
    }
    Ok(path.to_owned())
}

#[cfg(unix)]
fn private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// Generate an ed25519 key with no passphrase (the app must use it unattended
/// on every reconnect) in `dir`, named by its fingerprint. `ssh-keygen`
/// creates the private half readable only by this user, inside a directory
/// only this user can open. Returns the device ID.
pub(super) fn generate(dir: &Path, cancelled: &impl Fn() -> bool) -> Result<String> {
    let keys = identity::keys_dir(dir);
    private_directory(&keys).map_err(|source| Error::Store {
        path: keys.clone(),
        source,
    })?;
    let mut random = [0u8; 4];
    // Only names a scratch file inside a private directory; a clash fails
    // `ssh-keygen` rather than overwriting.
    let _ = getrandom::fill(&mut random);
    let scratch = keys.join(format!(".new-{:08x}", u32::from_le_bytes(random)));
    let public = |path: &Path| PathBuf::from(format!("{}.pub", path.display()));
    let made = (|| {
        let mut command = Command::new("ssh-keygen");
        command
            .args(["-q", "-t", "ed25519", "-N", "", "-C", COMMENT])
            .arg("-f")
            .arg(&scratch);
        match run(command, cancelled)? {
            (Some(0), _) => {}
            (code, _) => return Err(Error::Keygen(code)),
        }
        let id = fingerprint(&scratch, cancelled)?;
        let target = keys.join(&id);
        if target.exists() {
            return Err(Error::Invalid("generated key name"));
        }
        let moved = |from: &Path, to: &Path| {
            fs::rename(from, to).map_err(|source| Error::Store {
                path: to.to_owned(),
                source,
            })
        };
        moved(&scratch, &target)?;
        moved(&public(&scratch), &public(&target))?;
        Ok(id)
    })();
    if made.is_err() {
        let _ = fs::remove_file(&scratch);
        let _ = fs::remove_file(public(&scratch));
    }
    made
}

/// Private keys in `~/.ssh` that have a `.pub` beside them, for the settings
/// card to offer. Bounded; names only, nothing is read from the files.
pub(crate) fn detected(home: &Path) -> Vec<PathBuf> {
    const SCAN: usize = 64;
    const OFFERED: usize = 8;
    let Ok(entries) = fs::read_dir(home.join(".ssh")) else {
        return Vec::new();
    };
    let mut keys: Vec<PathBuf> = entries
        .flatten()
        .take(SCAN)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_none_or(|ext| ext != "pub"))
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .is_some_and(|name| !name.to_string_lossy().starts_with('.'))
                && PathBuf::from(format!("{}.pub", path.display())).is_file()
        })
        .collect();
    keys.sort();
    keys.truncate(OFFERED);
    keys
}

// Tests drive `ssh`, `ssh-keygen`, and `/bin/sh`; cloud devices are Unix-only.
#[cfg(all(test, unix))]
mod tests;
