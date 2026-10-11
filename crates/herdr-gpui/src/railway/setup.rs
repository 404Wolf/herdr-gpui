//! The blocking jobs behind Railway's Settings tab and its device picker row:
//! make a named key, pick the identity (that key or one the user already has),
//! reach `railway.new` headlessly with it, check for (and on approval install)
//! Herdr, and save the device.
//!
//! Making the local key is a separate, earlier step and creates nothing at
//! Railway. Setup then probes `railway.new` headlessly with the key: Railway
//! refuses that (exit 13) for a key with no VM, and only then does setup run
//! the one connection that creates it, on a pseudo-terminal (`first_use`).
//! Either way it ends by reaching the VM headlessly, so a reconnect later
//! behaves exactly like setup's last step.

use super::{
    ACCOUNT, Error, Result, first_use,
    identity::{self, Record},
    keys, refusal, ssh,
};
use crate::cloud::{self, CloudProvider, SavedDevice, install};
use std::path::{Path, PathBuf};

/// The session every Railway device attaches to.
const SESSION: &str = "default";
const NAME_LIMIT: usize = 40;

/// Which SSH identity setup uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    /// The key this app generated that no device uses yet.
    Generated,
    /// A private key the user already has, by absolute path.
    Existing(PathBuf),
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

/// Railway's own exit status for a headless connection by a key with no VM.
const REFUSED: i32 = 13;

/// The user's home directory, where `~/.ssh` is looked for.
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Make the key a new device will use, named `name`, and return its path.
/// Creates only a local key pair; nothing reaches Railway. Refuses to make a
/// second while one waits for a device to use it.
pub(crate) fn generate_key(name: &str, cancelled: &dyn Fn() -> bool) -> Result<PathBuf> {
    keys::check_tools()?;
    let dir = identity::state_dir()?;
    generate_key_in(&dir, home().as_deref(), name, &cancelled)
}

fn generate_key_in(
    dir: &Path,
    home: Option<&Path>,
    name: &str,
    cancelled: &impl Fn() -> bool,
) -> Result<PathBuf> {
    // Checking for a waiting key and making one is one step: two jobs (a
    // double click) must not each make their own.
    static GENERATING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one_at_a_time = GENERATING.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(waiting) = identity::pending(dir)? {
        return Err(Error::KeyWaiting(waiting.key_path()));
    }
    let made = keys::generate(&keys::key_directory(home, dir), name, cancelled)?;
    let record = Record {
        id: made.id,
        path: made.path.to_str().ok_or(Error::KeyPath)?.to_owned(),
        attached: false,
        fingerprint: Some(made.fingerprint),
    };
    identity::save(dir, record)?;
    Ok(made.path)
}

/// Pick the identity. A key is made only by [`generate_key`], never here.
fn resolve(dir: &Path, choice: Choice, cancelled: &impl Fn() -> bool) -> Result<Record> {
    match choice {
        Choice::Generated => identity::pending(dir)?.ok_or(Error::NoPendingKey),
        Choice::Existing(path) => {
            let path = keys::existing(&path)?;
            let id = keys::fingerprint(&path, cancelled)?;
            let path = path.to_str().ok_or(Error::KeyPath)?.to_owned();
            // A key already recorded follows the path just chosen, and keeps
            // whether a device uses it.
            Ok(match identity::load(dir, &id)? {
                Some(record) => Record { path, ..record },
                None => Record {
                    id,
                    path,
                    attached: false,
                    fingerprint: None,
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
    Ok(add_device_in(
        &dir,
        request,
        cancelled,
        report,
        cloud::load,
        cloud::save,
    )?)
}

fn add_device_in(
    dir: &Path,
    request: AddRequest,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
    load: impl FnOnce() -> cloud::Result<Vec<SavedDevice>>,
    save: impl FnOnce(SavedDevice) -> cloud::Result<()>,
) -> Result<SavedDevice> {
    let AddRequest { choice, install } = request;
    // Before a key is made or a VM could be created.
    keys::check_tools()?;
    let record = resolve(dir, choice, &cancelled)?;
    let key = record.key_path();
    if !key.is_file() {
        return Err(Error::KeyMissing(key));
    }
    // The file at the saved path must still be this device's key: a replaced
    // file or retargeted link would otherwise reach another VM.
    if keys::fingerprint(&key, &cancelled)? != record.id {
        return Err(Error::KeyChanged(key));
    }
    // One config for the whole job: every `ssh` run below is built from it
    // and it must outlive each of them.
    let config = ssh::Config::new(&key, &ssh::known_hosts(dir))?;
    report(cloud::Step::Connecting);
    // A failure from here leaves a generated key unattached: the next attempt
    // reuses it, so a VM already made for it is not lost.
    if !vm_exists(&config, cancelled)? {
        report(cloud::Step::Creating);
        create_vm(&config, cancelled)?;
        report(cloud::Step::Connecting);
        if !vm_exists(&config, cancelled)? {
            return Err(Error::CreateRefused(
                "the session ended without creating one".into(),
            ));
        }
    }
    ensure_herdr(|| Ok(config.command()), install, cancelled, report)?;
    attach(dir, record, load, save)
}

/// Whether `railway.new` has a VM for the key, by a bare headless `ssh -T`
/// with no remote command. This never creates a VM and never retries. A
/// refusal means "no VM" only when Railway says a terminal is needed; any
/// other refusal, such as the daily limit, is final.
fn vm_exists(config: &ssh::Config, cancelled: &dyn Fn() -> bool) -> Result<bool> {
    let finished = install::reach(config.command(), &cancelled)?;
    match finished.code {
        Some(0) => Ok(true),
        Some(REFUSED) => match refusal::refusal(&finished.output) {
            refusal::Refusal::NeedsTerminal => Ok(false),
            refusal::Refusal::Other => Err(Error::CreateRefused(failure_text(&finished.output))),
        },
        _ => Err(Error::Unreachable(failure_text(&finished.output))),
    }
}

fn failure_text(output: &str) -> String {
    if output.is_empty() {
        "the ssh command failed".into()
    } else {
        refusal::hide_links(output)
    }
}

/// The one connection that creates the VM, on a pseudo-terminal, for a key a
/// headless probe just found has none. Nothing is read from it but Railway's
/// own words if it refuses (its daily limit, for one): the probe that follows
/// says whether the VM exists.
fn create_vm(config: &ssh::Config, cancelled: &dyn Fn() -> bool) -> Result<()> {
    let mut session = config.terminal_command();
    session.arg(install::remote("true"));
    let finished = first_use::create_vm(session, cancelled)?;
    match finished.code {
        Some(0) => Ok(()),
        Some(REFUSED) => Err(Error::CreateRefused(failure_text(&finished.output))),
        _ => Err(Error::Unreachable(failure_text(&finished.output))),
    }
}

/// A failure's output tail can hold a link from Railway; it must not reach a
/// toast.
fn hidden<T>(result: cloud::Result<T>) -> Result<T> {
    result.map_err(|error| match error {
        cloud::Error::Install(text) => cloud::Error::Install(refusal::hide_links(&text)).into(),
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

/// Save the identity and the device once Herdr is ready on the VM. The
/// catalog is read first, and if saving the device fails the identity goes
/// back to exactly what it was.
fn attach(
    dir: &Path,
    record: Record,
    load: impl FnOnce() -> cloud::Result<Vec<SavedDevice>>,
    save: impl FnOnce(SavedDevice) -> cloud::Result<()>,
) -> Result<SavedDevice> {
    // Setting a device up again keeps its name.
    let known = load()?
        .into_iter()
        .find(|saved| saved.provider == CloudProvider::Railway && saved.id == record.id);
    let original = identity::load(dir, &record.id)?;
    let name = random_name();
    let device = SavedDevice {
        provider: CloudProvider::Railway,
        id: record.id.clone(),
        label: known
            .as_ref()
            .map_or_else(|| name.clone(), |saved| saved.label.clone()),
        account: ACCOUNT.to_owned(),
        machine: known.map_or(name, |saved| saved.machine),
        session: SESSION.to_owned(),
        enabled: true,
    };
    identity::save(
        dir,
        Record {
            attached: true,
            ..record.clone()
        },
    )?;
    if let Err(error) = save(device.clone()) {
        let _ = identity::restore(dir, &record.id, original);
        return Err(error.into());
    }
    Ok(device)
}

/// One saved device and where its key is, for the Settings tab.
#[derive(Debug)]
pub(crate) struct Device {
    pub(crate) saved: SavedDevice,
    /// The key's file, if the device has an identity.
    pub(crate) key_path: Option<PathBuf>,
    pub(crate) key_present: bool,
}

/// The generated key no device uses yet, which the next setup uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingKey {
    pub(crate) path: PathBuf,
    pub(crate) fingerprint: Option<String>,
}

/// What the Settings tab shows.
#[derive(Debug)]
pub(crate) struct Overview {
    pub(crate) devices: Vec<Device>,
    pub(crate) pending_key: Option<PendingKey>,
    /// Where a newly generated key will be created.
    pub(crate) new_key_directory: keys::KeyDirectory,
}

pub(crate) fn overview() -> Result<Overview> {
    let dir = identity::state_dir()?;
    overview_in(&dir, home().as_deref(), cloud::load()?)
}

fn overview_in(dir: &Path, home: Option<&Path>, saved: Vec<SavedDevice>) -> Result<Overview> {
    let records = identity::load_all(dir)?;
    let devices = saved
        .into_iter()
        .filter(|saved| saved.provider == CloudProvider::Railway)
        .map(|saved| {
            let key_path = records
                .iter()
                .find(|record| record.id == saved.id)
                .map(Record::key_path);
            Device {
                key_present: key_path.as_ref().is_some_and(|path| path.is_file()),
                key_path,
                saved,
            }
        })
        .collect();
    Ok(Overview {
        devices,
        pending_key: records
            .iter()
            .find(|record| !record.attached)
            .map(|record| PendingKey {
                path: record.key_path(),
                fingerprint: record.fingerprint.clone(),
            }),
        new_key_directory: keys::key_directory(home, dir),
    })
}

/// Forget one saved device and its identity. The VM is left alone at Railway
/// (it expires on its own) and no key file is ever deleted.
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
// The terminal session that creates a VM, against a stand-in for Railway.
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod create_tests;
