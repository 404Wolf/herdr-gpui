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

/// The `SHA256:…` word of `ssh-keygen -l` output such as
/// `256 SHA256:abc… comment (ED25519)`.
fn digest(output: &str) -> Option<String> {
    let word = output.lines().next()?.split_whitespace().nth(1)?;
    word.strip_prefix("SHA256:")?;
    (word.len() <= 128 && word.bytes().all(|b| b.is_ascii_graphic())).then(|| word.to_owned())
}

/// A device ID from `ssh-keygen -l` output.
#[cfg(test)]
pub(super) fn device_id(output: &str) -> Option<String> {
    id_of(&digest(output)?)
}

/// A device ID from a `SHA256:…` fingerprint: its first 16 letters and digits.
fn id_of(fingerprint: &str) -> Option<String> {
    let digest = fingerprint.strip_prefix("SHA256:")?;
    let id: String = digest
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(16)
        .collect();
    (id.len() == 16).then(|| format!("key-{id}"))
}

/// The full fingerprint (`SHA256:…`) of the key at `path`. An empty
/// passphrase is supplied so a protected key falls back to its `.pub` file
/// instead of prompting.
pub(super) fn fingerprint_text(path: &Path, cancelled: &impl Fn() -> bool) -> Result<String> {
    let mut command = Command::new("ssh-keygen");
    command
        .args(["-l", "-E", "sha256", "-P", ""])
        .arg("-f")
        .arg(path);
    match run(command, cancelled)? {
        (Some(0), output) => digest(&output).ok_or_else(|| Error::UnusableKey(path.to_owned())),
        _ => Err(Error::UnusableKey(path.to_owned())),
    }
}

/// The device ID for the key at `path`.
pub(super) fn fingerprint(path: &Path, cancelled: &impl Fn() -> bool) -> Result<String> {
    let text = fingerprint_text(path, cancelled)?;
    id_of(&text).ok_or_else(|| Error::UnusableKey(path.to_owned()))
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

/// The rule for key names, stated once for the UI and the backend alike.
pub(crate) const NAME_RULE: &str = "Use 1 to 64 letters, digits, dots, underscores or hyphens, not starting with a dot or hyphen and not ending in .pub.";

/// A key name that is one safe file name: no separators, traversal, hidden or
/// option-like names, and nothing that collides with a public key's name.
pub(crate) fn valid_name(name: &str) -> Result<()> {
    let plain = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['.', '-'])
        && !name.ends_with(".pub")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    plain.then_some(()).ok_or(Error::KeyName)
}

/// Where new keys go: the user's `~/.ssh` when it is a directory, else
/// Herdr's own private folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyDirectory {
    pub(crate) path: PathBuf,
    /// Whether this is the user's `~/.ssh`, whose keys are theirs to keep.
    pub(crate) user: bool,
}

pub(crate) fn key_directory(home: Option<&Path>, state: &Path) -> KeyDirectory {
    match home
        .map(|home| home.join(".ssh"))
        .filter(|ssh| ssh.is_dir())
    {
        Some(path) => KeyDirectory { path, user: true },
        None => KeyDirectory {
            path: identity::keys_dir(state),
            user: false,
        },
    }
}

/// A key just made: where, and its fingerprint.
#[derive(Debug)]
pub(super) struct Made {
    pub(super) path: PathBuf,
    pub(super) fingerprint: String,
}

fn exists(path: &Path) -> bool {
    // A symlink, even a dangling one, is a name already taken.
    fs::symlink_metadata(path).is_ok()
}

/// Generate an ed25519 key named `name` in `directory`, with no passphrase
/// (the app must use it unattended on every reconnect). Never replaces
/// anything: the pair is made under scratch names and then linked to its
/// final names, which fails if either is taken. `ssh-keygen` creates the
/// private half readable only by this user; Herdr's own folder is created
/// only this user can open, and `~/.ssh` is left as the user has it.
pub(super) fn generate(
    directory: &KeyDirectory,
    name: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Made> {
    valid_name(name)?;
    if !directory.user {
        private_directory(&directory.path).map_err(|source| Error::Store {
            path: directory.path.clone(),
            source,
        })?;
    }
    let target = directory.path.join(name);
    let public_target = identity::public_path(&target);
    if exists(&target) || exists(&public_target) {
        return Err(Error::KeyExists(target));
    }
    let mut random = [0u8; 4];
    // Only names a scratch file; a clash fails `ssh-keygen` rather than
    // overwriting.
    let _ = getrandom::fill(&mut random);
    let scratch = directory
        .path
        .join(format!(".herdr-new-{:08x}", u32::from_le_bytes(random)));
    let public_scratch = identity::public_path(&scratch);
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
        let fingerprint = fingerprint_text(&scratch, cancelled)?;
        place(&scratch, &target)?;
        if let Err(error) = place(&public_scratch, &public_target) {
            let _ = fs::remove_file(&target);
            return Err(error);
        }
        Ok(Made {
            path: target.clone(),
            fingerprint,
        })
    })();
    let _ = fs::remove_file(&scratch);
    let _ = fs::remove_file(&public_scratch);
    made
}

/// Give `from` the name `to` without replacing whatever has it. A link fails
/// if `to` exists; a file system without links falls back to a rename after
/// a last check.
fn place(from: &Path, to: &Path) -> Result<()> {
    let failed = |source: io::Error| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            Error::KeyExists(to.to_owned())
        } else {
            Error::Store {
                path: to.to_owned(),
                source,
            }
        }
    };
    match fs::hard_link(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(failed(error)),
        Err(_) if exists(to) => Err(Error::KeyExists(to.to_owned())),
        Err(_) => fs::rename(from, to).map_err(failed),
    }
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
