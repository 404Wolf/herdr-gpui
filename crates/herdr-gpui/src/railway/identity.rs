//! The SSH identity attached to each Railway device. The shared device list
//! holds no key material (and its schema is fixed), so the association lives
//! here, in a provider-private file next to it, keyed by the device ID. The ID
//! is derived from the key's fingerprint, so one key is one device. A record
//! only says where the private key is: forgetting it never touches the key.

use super::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const FILE: &str = "railway-identities.json";
const KEYS: &str = "railway-keys";
const VERSION: u32 = 2;
const LIMIT: u64 = 64 * 1024;
const MAX_RECORDS: usize = 64;
const PATH_LIMIT: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    /// The device ID: `key-` and part of the key's SHA-256 fingerprint.
    pub(crate) id: String,
    /// Absolute path of the private key.
    pub(crate) path: String,
    /// Whether a saved device uses this identity. A generated key that setup
    /// did not finish with stays unattached, and the next setup that asks for
    /// a generated key reuses it, so a retry reaches the same VM instead of
    /// spending another of Railway's three daily VMs.
    pub(crate) attached: bool,
    /// The key's full fingerprint (`SHA256:…`), for display; the device ID is
    /// the part of it that is checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) fingerprint: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,
    identities: Vec<Record>,
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
            && self.path.len() <= PATH_LIMIT
            && Path::new(&self.path).is_absolute()
            && !self.path.chars().any(char::is_control)
    }

    pub(crate) fn key_path(&self) -> PathBuf {
        PathBuf::from(&self.path)
    }
}

/// Herdr's own folder for keys it generates when there is no `~/.ssh`.
pub(super) fn keys_dir(dir: &Path) -> PathBuf {
    dir.join(KEYS)
}

/// Where the public half of the key at `path` is: `<path>.pub`. Appended to
/// the whole name, since key names may hold dots.
pub(super) fn public_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".pub");
    PathBuf::from(name)
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
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(Error::Json)?;
    // A file from before this format was settled holds no usable identity.
    if value.get("version").and_then(|version| version.as_u64()) != Some(u64::from(VERSION)) {
        return Ok(Vec::new());
    }
    let document: Document = serde_json::from_value(value).map_err(Error::Json)?;
    if document.identities.len() > MAX_RECORDS || !document.identities.iter().all(Record::valid) {
        return Err(Error::Invalid(FILE));
    }
    Ok(document.identities)
}

fn write_in(dir: &Path, identities: Vec<Record>) -> Result<()> {
    if identities.len() > MAX_RECORDS || !identities.iter().all(Record::valid) {
        return Err(Error::Invalid("Railway identity"));
    }
    let path = dir.join(FILE);
    let mut bytes = serde_json::to_vec_pretty(&Document {
        version: VERSION,
        identities,
    })
    .map_err(Error::Json)?;
    bytes.push(b'\n');
    // Refuse what could not be read back, before replacing what can.
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Invalid(FILE));
    }
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    // Created 0600; it names where private keys are.
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(io_error(dir))?;
    file.write_all(&bytes).map_err(io_error(&path))?;
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

/// Set the record for `id` to `record`, or remove it when there is none: the
/// way to put an identity back exactly as it was.
pub(super) fn restore(dir: &Path, id: &str, record: Option<Record>) -> Result<()> {
    transaction(dir, |identities| {
        identities.retain(|saved| saved.id != id);
        identities.extend(record);
        Ok(())
    })
}

/// Add `record` or replace the one with its ID.
pub(super) fn save(dir: &Path, record: Record) -> Result<()> {
    let id = record.id.clone();
    restore(dir, &id, Some(record))
}

/// The generated key setup did not finish with, if one is waiting. Only
/// generated keys are saved before a device uses them.
pub(super) fn pending(dir: &Path) -> Result<Option<Record>> {
    Ok(load_all(dir)?.into_iter().find(|record| !record.attached))
}

/// Forget the identity for `id`. The key files stay where they are.
pub(super) fn forget(dir: &Path, id: &str) -> Result<()> {
    restore(dir, id, None)
}

#[cfg(test)]
impl Record {
    /// A record for a key at `path`, no device using it yet, for tests.
    pub(crate) fn unattached(id: &str, path: &str) -> Self {
        Self {
            id: id.into(),
            path: path.into(),
            attached: false,
            fingerprint: None,
        }
    }
}

// Tests drive `ssh`, `ssh-keygen`, and `/bin/sh`; cloud devices are Unix-only.
#[cfg(all(test, unix))]
mod tests;
