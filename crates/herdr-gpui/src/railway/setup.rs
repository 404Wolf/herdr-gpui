//! The blocking jobs behind Railway's Settings tab and its device picker row:
//! pick the identity (generate, borrow, or the one a device already has),
//! reach `railway.new` headlessly with it, check for (and on approval install)
//! Herdr, and save the device. Also what the tab shows and what removal does.
//!
//! Setup never creates the VM. Railway refuses a scripted first connection
//! for a new key (exit 13), so the user makes that one connection themselves
//! in an interactive terminal, with a command shown here; setup then reaches
//! the existing VM headlessly. Nothing here pretends to be a terminal.

use super::{
    ACCOUNT, Error, Result,
    identity::{self, Record, Source},
    keys, manifest, ssh,
};
use crate::cloud::{self, CloudProvider, SavedDevice, install};
use std::path::{Path, PathBuf};

/// The session every Railway device attaches to.
const SESSION: &str = "default";
const NAME_LIMIT: usize = 40;

/// Which SSH identity setup uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    /// A new key this app generates and owns, or the one a previous setup
    /// generated and did not finish with.
    Generate,
    /// A private key the user already has, by absolute path.
    Existing(PathBuf),
    /// The identity an already saved device has, to set that device up again.
    Saved(String),
}

/// What setting up a Railway device needs.
pub(crate) struct AddRequest {
    pub(crate) choice: Choice,
    /// The user approved running Herdr's installer if the VM lacks it.
    pub(crate) install: bool,
}

/// A default name for a new device.
pub(crate) fn random_name() -> String {
    cloud::names::random("railway", NAME_LIMIT)
}

/// Railway's own exit status for a refused scripted first connection.
const REFUSED: i32 = 13;

/// The command the user runs once in a terminal of their own to create the VM
/// for `key`. `-F /dev/null` ignores their SSH config, so no other identity,
/// host alias, or forwarding of theirs can decide which VM they create; the
/// session itself stays fully interactive. Quoted for a POSIX shell.
pub(crate) fn first_use_command(key: &Path) -> String {
    let quoted = key.display().to_string().replace('\'', "'\\''");
    format!("ssh -F /dev/null -i '{quoted}' -o IdentitiesOnly=yes railway.new")
}

/// Generate the key a new device will use, if none is waiting, and return its
/// path, so the user can make the first connection with it before setup.
pub(crate) fn generate_key(cancelled: &dyn Fn() -> bool) -> Result<PathBuf> {
    keys::check_tools()?;
    let dir = identity::state_dir()?;
    let record = resolve(&dir, Choice::Generate, &cancelled)?;
    Ok(record.key_path(&dir))
}

/// Pick the identity, creating a key only for `Generate` and only when no
/// unfinished one is waiting.
fn resolve(dir: &Path, choice: Choice, cancelled: &impl Fn() -> bool) -> Result<Record> {
    match choice {
        Choice::Saved(id) => identity::load(dir, &id)?.ok_or(Error::NoIdentity),
        Choice::Generate => {
            // Checking for a waiting key and making one is one step: two
            // jobs (a double click, or Generate beside a setup) must not
            // each make their own.
            static GENERATING: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let _one_at_a_time = GENERATING.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(record) = identity::unattached_generated(dir)? {
                return Ok(record);
            }
            let id = keys::generate(dir, cancelled)?;
            let record = Record {
                id: id.clone(),
                source: Source::Generated,
                attached: false,
                window_ends_by: None,
                claimed: false,
            };
            // A key nothing records can never be reused or deleted: take it
            // back if the record cannot be saved.
            if let Err(error) = identity::save(dir, record.clone()) {
                let _ = identity::remove_generated(dir, &id);
                return Err(error);
            }
            Ok(record)
        }
        Choice::Existing(path) => {
            let path = keys::existing(&path)?;
            let id = keys::fingerprint(&path, cancelled)?;
            let path = path.to_str().ok_or(Error::KeyPath)?.to_owned();
            // A key this app already records keeps its owner: pointing at a
            // generated key's file must not turn it into a borrowed one. A
            // borrowed key may have moved, so it follows the path just chosen.
            Ok(match identity::load(dir, &id)? {
                Some(record) if record.owned() => record,
                Some(record) => Record {
                    source: Source::Borrowed { path },
                    ..record
                },
                None => Record {
                    id,
                    source: Source::Borrowed { path },
                    attached: false,
                    window_ends_by: None,
                    claimed: false,
                },
            })
        }
    }
}

/// The whole job for `cloud::Jobs`.
pub(crate) fn add_device(
    request: AddRequest,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
) -> crate::Result<SavedDevice> {
    let dir = identity::state_dir()?;
    Ok(add_device_in(&dir, request, cancelled, report)?)
}

fn add_device_in(
    dir: &Path,
    request: AddRequest,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
) -> Result<SavedDevice> {
    let AddRequest { choice, install } = request;
    // Before a key is made or a VM could be created.
    keys::check_tools()?;
    let record = resolve(dir, choice, &cancelled)?;
    let key = record.key_path(dir);
    if !key.is_file() {
        return Err(Error::KeyMissing(key));
    }
    // The file at the saved path must still be this device's key: a replaced
    // file or retargeted link would otherwise reach another VM.
    if keys::fingerprint(&key, &cancelled)? != record.id {
        return Err(Error::KeyChanged(key));
    }
    let known_hosts = ssh::known_hosts(dir);
    // One config for the whole job: every `ssh` run below is built from it
    // and it must outlive each of them.
    let config = ssh::Config::new(&key, &known_hosts)?;
    let session = || Ok(config.command());
    report(cloud::Step::Connecting);
    // A failure from here leaves a generated key unattached: the next attempt
    // reuses it, so the user's first connection is not wasted.
    let deadline = reach(config.command(), &key, cancelled)?;
    ensure_herdr(session, install, cancelled, report)?;
    let device = attach(dir, record, deadline, cloud::load, cloud::save)?;
    // A device switched off after a refusal connects again once the user set
    // it up again.
    super::connect::resume(&device.id);
    Ok(device)
}

/// Reach the VM headlessly and read its deadline. Railway refuses this for a
/// key whose VM does not exist yet, which is reported with the command that
/// makes it; it never creates a VM or retries.
fn reach(
    session: std::process::Command,
    key: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<u64>> {
    let finished = install::reach(session, &cancelled)?;
    match finished.code {
        Some(0) => {
            let now = identity::now();
            let deadline = manifest::deadline(&finished.output, now);
            if deadline.is_some_and(|deadline| deadline <= now) {
                return Err(Error::Expired);
            }
            Ok(deadline)
        }
        Some(REFUSED) => Err(Error::FirstUseRequired {
            command: first_use_command(key),
        }),
        _ => Err(Error::Unreachable(if finished.output.is_empty() {
            "the ssh command failed".into()
        } else {
            manifest::hide_links(&finished.output)
        })),
    }
}

/// A failure's output tail can hold Railway's manifest, and with it a claim
/// link; it must not reach a toast.
fn hidden<T>(result: cloud::Result<T>) -> Result<T> {
    result.map_err(|error| match error {
        cloud::Error::Install(text) => cloud::Error::Install(manifest::hide_links(&text)).into(),
        error => error.into(),
    })
}

/// Make sure the VM has a Herdr the bridge can run, installing it only when
/// the user approved. `session` builds a fresh `ssh` command for each remote
/// command; setup is the only caller, so no reconnect ever installs.
fn ensure_herdr(
    session: impl Fn() -> Result<std::process::Command>,
    install: bool,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
) -> Result<()> {
    if hidden(install::installed(session()?, &cancelled))? {
        return Ok(());
    }
    if !install {
        return Err(cloud::Error::Install(
            "Herdr is not installed on the Railway VM; allow installing it and try again".into(),
        )
        .into());
    }
    report(cloud::Step::InstallingHerdr);
    hidden(install::install(session()?, &cancelled))?;
    // The installer's own success is not proof the bridge will find it.
    if !hidden(install::installed(session()?, &cancelled))? {
        return Err(cloud::Error::Install(
            "the installer finished but Herdr is not on the expected paths".into(),
        )
        .into());
    }
    Ok(())
}

/// Save the identity and the device once Herdr is ready on the VM.
fn attach(
    dir: &Path,
    record: Record,
    deadline: Option<u64>,
    load: impl FnOnce() -> cloud::Result<Vec<SavedDevice>>,
    save: impl FnOnce(SavedDevice) -> cloud::Result<()>,
) -> Result<SavedDevice> {
    let reached = record.clone().reached(identity::now(), deadline);
    identity::save(dir, reached.clone())?;
    // Setting a device up again keeps its name.
    let known = load()?
        .into_iter()
        .find(|saved| saved.provider == CloudProvider::Railway && saved.id == reached.id);
    let name = random_name();
    let device = SavedDevice {
        provider: CloudProvider::Railway,
        id: reached.id.clone(),
        label: known
            .as_ref()
            .map_or_else(|| name.clone(), |saved| saved.label.clone()),
        account: ACCOUNT.to_owned(),
        machine: known.map_or(name, |saved| saved.machine),
        session: SESSION.to_owned(),
        enabled: true,
    };
    if let Err(error) = save(device.clone()) {
        // Not attached to anything: a generated key stays reusable.
        let _ = identity::save(
            dir,
            Record {
                attached: false,
                ..reached
            },
        );
        return Err(error.into());
    }
    Ok(device)
}

/// One saved device and what identifies its key, for the Settings tab.
#[derive(Debug)]
pub(crate) struct Device {
    pub(crate) saved: SavedDevice,
    pub(crate) record: Option<Record>,
    pub(crate) key_present: bool,
}

/// What the Settings tab shows.
#[derive(Debug)]
pub(crate) struct Overview {
    pub(crate) devices: Vec<Device>,
    /// The generated key no device uses yet, which the next setup reuses.
    pub(crate) pending_key: Option<PathBuf>,
    /// Keys in the user's `~/.ssh` the tab can offer.
    pub(crate) detected: Vec<PathBuf>,
}

pub(crate) fn overview() -> Result<Overview> {
    let dir = identity::state_dir()?;
    let records = identity::load_all(&dir)?;
    let devices = cloud::load()?
        .into_iter()
        .filter(|saved| saved.provider == CloudProvider::Railway)
        .map(|saved| {
            let record = records.iter().find(|record| record.id == saved.id).cloned();
            let key_present = record
                .as_ref()
                .is_some_and(|record| record.key_path(&dir).is_file());
            Device {
                saved,
                record,
                key_present,
            }
        })
        .collect();
    let detected = std::env::var_os("HOME")
        .map(|home| keys::detected(Path::new(&home)))
        .unwrap_or_default();
    Ok(Overview {
        devices,
        pending_key: records
            .iter()
            .find(|record| record.owned() && !record.attached)
            .map(|record| record.key_path(&dir)),
        detected,
    })
}

/// Say the user claimed the VM, so no build window applies to it.
pub(crate) fn mark_claimed(id: &str) -> Result<()> {
    let dir = identity::state_dir()?;
    let record = identity::load(&dir, id)?.ok_or(Error::NoIdentity)?;
    identity::save(
        &dir,
        Record {
            claimed: true,
            ..record
        },
    )
}

/// Forget one saved device and its identity. The VM is left alone at Railway
/// (it expires on its own); a generated key is deleted with the device, and a
/// borrowed key's file is never touched.
pub(crate) fn forget_device(id: &str) -> Result<()> {
    let dir = identity::state_dir()?;
    cloud::remove(CloudProvider::Railway, id)?;
    identity::forget(&dir, id)
}

/// Expand a leading `~/` in a typed path.
pub(crate) fn expand(text: &str) -> PathBuf {
    match (text.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => Path::new(&home).join(rest),
        _ => PathBuf::from(text),
    }
}

// Tests drive `ssh`, `ssh-keygen`, and `/bin/sh`; cloud devices are Unix-only.
#[cfg(all(test, unix))]
mod tests;
