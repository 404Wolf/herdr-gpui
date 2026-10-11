//! SSH keys for Railway devices, through the system's `ssh-keygen`: reading a
//! key's fingerprint (which names its device) and generating a new one. The
//! GUI never parses or holds private key bytes. Runs are bounded, take no
//! terminal input, and observe cancellation.

use super::{Error, Result, identity};
use std::{
    fs,
    io::{self, Read, Write},
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

fn run(
    mut command: Command,
    input: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<(Option<i32>, String)> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(Error::KeygenMissing)?;
    // A short public key fits the pipe buffer; closing stdin ends the input.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        // Checked first, so a cancel request wins over a child that just ended.
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Cloud(crate::cloud::Error::Cancelled));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => return Err(Error::KeygenMissing(error)),
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

/// The full fingerprint (`SHA256:…`) of the key at `path`, taken from the
/// private key itself. Reading it through `ssh-keygen -l -f <key>` would trust
/// a `.pub` beside it, which stays behind when the private file is replaced.
/// Only a passphrase-protected key, which cannot be read unattended, falls back
/// to its `.pub`.
pub(super) fn fingerprint_text(path: &Path, cancelled: &impl Fn() -> bool) -> Result<String> {
    let unusable = || Error::UnusableKey(path.to_owned());
    let mut derive = Command::new("ssh-keygen");
    derive.args(["-y", "-P", ""]).arg("-f").arg(path);
    let mut list = Command::new("ssh-keygen");
    list.args(["-l", "-E", "sha256", "-f", "-"]);
    let output = match run(derive, "", cancelled)? {
        (Some(0), public) => run(list, &public, cancelled)?,
        _ => {
            let mut list = Command::new("ssh-keygen");
            list.args(["-l", "-E", "sha256", "-f"])
                .arg(identity::public_path(path));
            run(list, "", cancelled)?
        }
    };
    match output {
        (Some(0), output) => digest(&output).ok_or_else(unusable),
        _ => Err(unusable()),
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

/// A key just made: where, its full fingerprint, and the device ID that
/// fingerprint names.
#[derive(Debug)]
pub(super) struct Made {
    pub(super) path: PathBuf,
    pub(super) fingerprint: String,
    pub(super) id: String,
}

fn exists(path: &Path) -> bool {
    // A symlink, even a dangling one, is a name already taken.
    fs::symlink_metadata(path).is_ok()
}

/// Generate an ed25519 key named `name` in `directory`, with no passphrase
/// (the app must use it unattended on every reconnect). Never replaces
/// anything: the pair is made inside a scratch directory this call owns, then
/// hard-linked to its final names, which fails if either is taken. Only that
/// scratch directory is ever removed. `ssh-keygen` creates the private half
/// readable only by this user; Herdr's own folder is created only this user
/// can open, and `~/.ssh` is left as the user has it.
pub(super) fn generate(
    directory: &KeyDirectory,
    name: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<Made> {
    valid_name(name)?;
    let store = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Store { path, source }
    };
    if !directory.user {
        private_directory(&directory.path).map_err(store(&directory.path))?;
    }
    let target = directory.path.join(name);
    let public_target = identity::public_path(&target);
    if exists(&target) || exists(&public_target) {
        return Err(Error::KeyExists(target));
    }
    // Beside the final names, so linking never crosses a file system.
    let scratch = tempfile::Builder::new()
        .prefix(".herdr-new-")
        .tempdir_in(&directory.path)
        .map_err(store(&directory.path))?;
    let key = scratch.path().join("key");
    let mut command = Command::new("ssh-keygen");
    command
        .args(["-q", "-t", "ed25519", "-N", "", "-C", COMMENT])
        .arg("-f")
        .arg(&key);
    match run(command, "", cancelled)? {
        (Some(0), _) => {}
        (code, _) => return Err(Error::Keygen(code)),
    }
    let fingerprint = fingerprint_text(&key, cancelled)?;
    let id = id_of(&fingerprint).ok_or_else(|| Error::UnusableKey(target.clone()))?;
    place(&key, &target)?;
    if let Err(error) = place(&identity::public_path(&key), &public_target) {
        // The private half was linked just above, by this call.
        let _ = fs::remove_file(&target);
        return Err(error);
    }
    Ok(Made {
        path: target,
        fingerprint,
        id,
    })
}

/// Give `from` the name `to` without replacing whatever has it: a hard link
/// fails if `to` exists, atomically. There is no fallback; a file system
/// without hard links makes key generation fail rather than risk an overwrite.
fn place(from: &Path, to: &Path) -> Result<()> {
    fs::hard_link(from, to).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            Error::KeyExists(to.to_owned())
        } else {
            Error::Store {
                path: to.to_owned(),
                source,
            }
        }
    })
}

// Tests drive `ssh`, `ssh-keygen`, and `/bin/sh`; cloud devices are Unix-only.
#[cfg(all(test, unix))]
mod tests;
