//! The SSH identity attached to each Railway device. The shared device list
//! holds no key material (and its schema is fixed), so the association lives
//! here, in a provider-private file next to it, keyed by the device ID. A
//! device ID is derived from the key's fingerprint, so one key is one device,
//! and a record says where the private key is and who owns it:
//!
//! - `Borrowed`: a key the user already had. Only its path is saved; the file
//!   is never copied, modified, or deleted.
//! - `Generated`: a key this app made under a name the user chose, in `~/.ssh`
//!   when that exists, else in a private directory beside the state files. The
//!   exact path and fingerprint are saved. One in the state folder is Herdr's
//!   own and goes with its device; one in `~/.ssh` is the user's to keep, and
//!   is deleted only when they explicitly ask.

use super::{Error, Result, WINDOW_SECONDS};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const FILE: &str = "railway-identities.json";
const KEYS: &str = "railway-keys";
const LIMIT: u64 = 64 * 1024;
const MAX_RECORDS: usize = 64;
const PATH_LIMIT: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Source {
    /// `path` is where the key was created; records from before keys were
    /// named have none, and their key is `<id>` in the state folder.
    Generated {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    Borrowed {
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    /// The device ID: `key-` and part of the key's SHA-256 fingerprint.
    pub(crate) id: String,
    pub(crate) source: Source,
    /// Whether a saved device uses this identity. A generated key that setup
    /// did not finish with stays unattached, and the next setup that asks for
    /// a generated key reuses it, so a retry reaches the same VM instead of
    /// spending another of Railway's three daily VMs.
    pub(crate) attached: bool,
    /// Unix seconds when the VM's build window ends: Railway's own
    /// `build_expires_at` when its manifest gave one, else an upper bound
    /// (the VM was created no later than setup last reached it).
    pub(crate) window_ends_by: Option<u64>,
    /// The user said they claimed the VM, so it has no build window.
    pub(crate) claimed: bool,
    /// The key's full fingerprint (`SHA256:…`) as it was when saved, for
    /// display; the device ID is the part of it that is checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fingerprint: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    identities: Vec<Record>,
}

pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Device IDs become file names and endpoint IDs: plain characters only.
pub(super) fn valid_id(id: &str) -> bool {
    id.len() <= 64
        && id
            .strip_prefix("key-")
            .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_alphanumeric()))
}

impl Record {
    fn valid(&self) -> bool {
        valid_id(&self.id)
            && match &self.source {
                Source::Generated { path: None } => true,
                Source::Generated { path: Some(path) } | Source::Borrowed { path } => {
                    path.len() <= PATH_LIMIT
                        && Path::new(path).is_absolute()
                        && !path.chars().any(char::is_control)
                }
            }
    }

    /// The private key file.
    pub(crate) fn key_path(&self, dir: &Path) -> PathBuf {
        match &self.source {
            Source::Generated { path: None } => keys_dir(dir).join(&self.id),
            Source::Generated { path: Some(path) } | Source::Borrowed { path } => {
                PathBuf::from(path)
            }
        }
    }

    /// A key this app made, which it may delete (see [`Self::kept`]).
    pub(crate) fn owned(&self) -> bool {
        matches!(self.source, Source::Generated { .. })
    }

    /// A generated key outside Herdr's own folder, in the user's `~/.ssh`:
    /// removing the device keeps the file unless they ask for it to go.
    pub(crate) fn kept(&self, dir: &Path) -> bool {
        self.owned() && !self.key_path(dir).starts_with(keys_dir(dir))
    }

    /// Whether connecting again could land on a VM other than the one setup
    /// reached: its build window has ended and the user has not said it was
    /// claimed. Checked on every reconnect; never guessed from attach time.
    pub(crate) fn window_ended(&self, now: u64) -> bool {
        !self.claimed && self.window_ends_by.is_none_or(|end| now > end)
    }

    /// The record after setup reached the VM at `now`. `deadline` is the
    /// one Railway reported. Without one, the window is bounded by assuming
    /// the VM was created no later than now, and never extends a deadline
    /// already recorded for it that has not passed: the same VM does not get
    /// longer by being reached again.
    pub(crate) fn reached(mut self, now: u64, deadline: Option<u64>) -> Self {
        let bound = now.saturating_add(WINDOW_SECONDS);
        self.attached = true;
        self.window_ends_by = Some(deadline.unwrap_or_else(|| match self.window_ends_by {
            Some(earlier) if earlier > now => earlier.min(bound),
            _ => bound,
        }));
        self
    }
}

pub(super) fn keys_dir(dir: &Path) -> PathBuf {
    dir.join(KEYS)
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> Error + '_ {
    move |source| Error::Store {
        path: path.to_owned(),
        source,
    }
}

fn read_in(dir: &Path) -> Result<Vec<Record>> {
    let path = dir.join(FILE);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(&path)(error)),
    };
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error(&path))?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Invalid(FILE));
    }
    let document: Document = serde_json::from_slice(&bytes).map_err(Error::Json)?;
    if document.version != 1
        || document.identities.len() > MAX_RECORDS
        || !document.identities.iter().all(Record::valid)
    {
        return Err(Error::Invalid(FILE));
    }
    Ok(document.identities)
}

fn write_in(dir: &Path, identities: Vec<Record>) -> Result<()> {
    if identities.len() > MAX_RECORDS || !identities.iter().all(Record::valid) {
        return Err(Error::Invalid("Railway identity"));
    }
    let path = dir.join(FILE);
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    // Created 0600; it names where private keys are.
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(io_error(dir))?;
    serde_json::to_writer_pretty(
        &mut file,
        &Document {
            version: 1,
            identities,
        },
    )
    .map_err(Error::Json)?;
    file.write_all(b"\n").map_err(io_error(&path))?;
    file.as_file().sync_all().map_err(io_error(&path))?;
    file.persist(&path)
        .map_err(|error| io_error(&path)(error.error))?;
    Ok(())
}

/// Every read-modify-write runs under this lock: windows share one process.
fn transaction<T>(dir: &Path, work: impl FnOnce(&mut Vec<Record>) -> Result<T>) -> Result<T> {
    static ACCESS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ACCESS.lock().unwrap_or_else(|error| error.into_inner());
    let mut identities = read_in(dir)?;
    let before = identities.clone();
    let result = work(&mut identities)?;
    if identities != before {
        write_in(dir, identities)?;
    }
    Ok(result)
}

/// The directory these files live in.
pub(super) fn state_dir() -> Result<PathBuf> {
    crate::preferences::state_dir().ok_or(Error::Cloud(crate::cloud::Error::StateDirectory))
}

pub(super) fn load_all(dir: &Path) -> Result<Vec<Record>> {
    transaction(dir, |identities| Ok(identities.clone()))
}

pub(super) fn load(dir: &Path, id: &str) -> Result<Option<Record>> {
    Ok(load_all(dir)?.into_iter().find(|record| record.id == id))
}

/// Add `record` or replace the one with its ID.
pub(super) fn save(dir: &Path, record: Record) -> Result<()> {
    transaction(dir, |identities| {
        match identities.iter_mut().find(|saved| saved.id == record.id) {
            Some(saved) => *saved = record,
            None => identities.push(record),
        }
        Ok(())
    })
}

/// The generated key setup did not finish with, if one is waiting.
pub(super) fn unattached_generated(dir: &Path) -> Result<Option<Record>> {
    Ok(load_all(dir)?
        .into_iter()
        .find(|record| record.owned() && !record.attached))
}

/// Forget the identity for `id`. A key in Herdr's own folder is deleted with
/// it. One generated in the user's `~/.ssh` is kept unless `delete_kept` says
/// they asked for it to go; a borrowed key's file is never touched.
pub(super) fn forget(dir: &Path, home: Option<&Path>, id: &str, delete_kept: bool) -> Result<()> {
    let removed = transaction(dir, |identities| {
        let index = identities.iter().position(|record| record.id == id);
        Ok(index.map(|index| identities.remove(index)))
    })?;
    match removed.filter(Record::owned) {
        Some(record) if delete_kept || !record.kept(dir) => {
            remove_generated(dir, home, &record.key_path(dir))
        }
        _ => Ok(()),
    }
}

/// Where the public half of the key at `path` is: `<path>.pub`. Appended to
/// the whole name, since key names may hold dots.
pub(super) fn public_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".pub");
    PathBuf::from(name)
}

/// Whether `path` is somewhere this app creates keys, with a name it could
/// have given: a saved record is a file the user can edit, and must never be
/// able to point a deletion at an arbitrary file.
fn ours(dir: &Path, home: Option<&Path>, path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    let parent = path.parent();
    name.is_some_and(|name| super::keys::valid_name(name).is_ok())
        && (parent == Some(keys_dir(dir).as_path())
            || home.is_some_and(|home| parent == Some(home.join(".ssh").as_path())))
}

/// Delete the key pair this app generated at `path`, if it is where and named
/// as it would have made it. Only called for a key it made itself.
pub(super) fn remove_generated(dir: &Path, home: Option<&Path>, path: &Path) -> Result<()> {
    if !ours(dir, home, path) {
        tracing::warn!(
            category = "railway_identity",
            "Not deleting a key outside Herdr's key locations"
        );
        return Ok(());
    }
    for path in [public_path(path), path.to_owned()] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::KeyFile {
                    path,
                    source: error,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
impl Record {
    /// A record no device uses yet, for tests.
    pub(crate) fn unattached(id: &str, source: Source) -> Self {
        Self {
            id: id.into(),
            source,
            attached: false,
            window_ends_by: None,
            claimed: false,
            fingerprint: None,
        }
    }
}

// Tests drive `ssh`, `ssh-keygen`, and `/bin/sh`; cloud devices are Unix-only.
#[cfg(all(test, unix))]
mod tests;
