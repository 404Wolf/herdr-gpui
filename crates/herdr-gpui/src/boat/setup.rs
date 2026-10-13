//! Blocking work behind Boat's Settings tab: record a created sandbox before
//! setup, wait, check/install Herdr, and summarize the configured account.

use super::{
    Result, Settings,
    api::{self, Readiness},
    gateway,
    http::Request,
    store,
};
use crate::cloud::{self, CloudProvider, SavedDevice, install};
use crate::github::Store;
use secrecy::SecretString;

const SESSION: &str = "default";
const NAME_LIMIT: usize = 40;

pub(crate) struct AddRequest {
    pub(crate) settings: Settings,
    pub(crate) target: Target,
    pub(crate) install: bool,
}

pub(crate) enum Target {
    Create(String),
    Saved(SavedDevice),
}

pub(crate) fn random_name() -> String {
    cloud::names::random("herdr", NAME_LIMIT)
}

fn step(readiness: &Readiness) -> cloud::Step {
    match readiness {
        Readiness::Pending(state) => cloud::Step::Building(state.clone()),
        _ => cloud::Step::Starting,
    }
}

pub(crate) fn add_device(
    request: AddRequest,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
) -> crate::Result<SavedDevice> {
    if cancelled() {
        return Err(super::Error::Cancelled.into());
    }
    let AddRequest {
        settings,
        target,
        install,
    } = request;
    if let Target::Saved(device) = &target
        && (device.provider != CloudProvider::Boat || device.account != settings.account())
    {
        return Err(super::Error::Account.into());
    }
    gateway::check_ssh()?;
    let key = store::key(&settings)?;
    let request = Request {
        settings: &settings,
        key: &key,
    };
    finish_setup(
        &request,
        target,
        cancelled,
        report,
        |device| Ok(cloud::save(device)?),
        |sandbox| ensure_herdr(&request, sandbox, install, cancelled, report),
    )
}

fn save_device(
    device: &SavedDevice,
    save: &mut impl FnMut(SavedDevice) -> crate::Result<()>,
) -> Result<()> {
    save(device.clone()).map_err(|source| super::Error::SaveDevice {
        id: device.id.clone(),
        source: Box::new(source),
    })
}

fn new_device(settings: &Settings, sandbox: &api::Sandbox, label: String) -> SavedDevice {
    SavedDevice {
        provider: CloudProvider::Boat,
        id: sandbox.id.clone(),
        label,
        account: settings.account(),
        machine: sandbox.name.clone(),
        session: SESSION.to_owned(),
        enabled: true,
    }
}

fn finish_setup(
    request: &Request,
    target: Target,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
    mut save: impl FnMut(SavedDevice) -> crate::Result<()>,
    prepare: impl FnOnce(&api::Sandbox) -> crate::Result<()>,
) -> crate::Result<SavedDevice> {
    let mut device = match target {
        Target::Create(name) => {
            report(cloud::Step::Creating);
            // Cancellation must not lose a billable resource whose ID we know.
            // Save it even if cancellation arrived while create was in flight.
            let created = request.create(&name, cancelled, |created| {
                save_device(
                    &new_device(request.settings, created, name.clone()),
                    &mut save,
                )
            })?;
            new_device(request.settings, &created, name)
        }
        Target::Saved(device) => device,
    };
    let setup_error = |source| super::Error::Setup {
        id: device.id.clone(),
        source: Box::new(source),
    };
    let sandbox = api::wait_ready(request, &device.id, cancelled, |readiness| {
        report(step(readiness));
    })
    .map_err(crate::Error::from)
    .map_err(&setup_error)?;
    prepare(&sandbox).map_err(setup_error)?;
    device.machine = sandbox.name;
    save_device(&device, &mut save)?;
    Ok(device)
}

fn ensure_herdr(
    request: &Request,
    sandbox: &api::Sandbox,
    install: bool,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(cloud::Step),
) -> crate::Result<()> {
    report(cloud::Step::CheckingHerdr);
    let installed = gateway::prepare(request, &sandbox.id)?
        .with_command(|command| install::installed(command, &cancelled))?;
    if !installed {
        if !install {
            return Err(cloud::Error::Install(format!(
                "Herdr is not installed in Boat sandbox {}; enable Install Herdr and use Finish setup",
                sandbox.id
            ))
            .into());
        }
        report(cloud::Step::InstallingHerdr);
        gateway::prepare(request, &sandbox.id)?
            .with_command(|command| install::install(command, &cancelled))?;
        if !gateway::prepare(request, &sandbox.id)?
            .with_command(|command| install::installed(command, &cancelled))?
        {
            return Err(cloud::Error::Install(
                "the installer finished but Herdr is not on the expected paths".into(),
            )
            .into());
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Missing,
    Saved,
    Environment,
}

#[derive(Debug)]
pub(crate) struct Overview {
    pub(crate) key: Result<Key>,
    pub(crate) sandboxes: Option<Result<usize>>,
    pub(crate) devices: Vec<SavedDevice>,
}

pub(crate) fn overview(settings: &Settings) -> Result<Overview> {
    let account = settings.account();
    let devices = cloud::load()?
        .into_iter()
        .filter(|saved| saved.provider == CloudProvider::Boat && saved.account == account)
        .collect();
    let key = if std::env::var_os(store::VARIABLE).is_some() {
        Ok(Key::Environment)
    } else {
        store::saved(settings.store).map(|saved| {
            if saved.is_some() {
                Key::Saved
            } else {
                Key::Missing
            }
        })
    };
    Ok(account_overview(devices, key, || {
        let key = store::key(settings)?;
        Request {
            settings,
            key: &key,
        }
        .sandboxes()
        .map(|sandboxes| sandboxes.len())
    }))
}

fn account_overview(
    devices: Vec<SavedDevice>,
    key: Result<Key>,
    list: impl FnOnce() -> Result<usize>,
) -> Overview {
    let sandboxes = matches!(key, Ok(Key::Saved | Key::Environment)).then(list);
    Overview {
        key,
        sandboxes,
        devices,
    }
}

pub(crate) fn save_key(store: Store, key: Option<&SecretString>) -> Result<()> {
    store::save(store, key)
}

pub(crate) fn forget_device(id: &str) -> Result<()> {
    Ok(cloud::remove(CloudProvider::Boat, id)?)
}

#[cfg(test)]
mod tests;
