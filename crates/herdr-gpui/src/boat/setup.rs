//! Blocking work behind Boat's Settings tab: create, wait, check/install Herdr,
//! save the device, and summarize the configured account.

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
    pub(crate) name: String,
    pub(crate) install: bool,
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
        name,
        install,
    } = request;
    gateway::check_ssh()?;
    let key = store::key(&settings)?;
    let request = Request {
        settings: &settings,
        key: &key,
    };
    report(cloud::Step::Creating);
    let created = request.create(&name, cancelled)?;
    let sandbox = api::wait_ready(&request, &created.id, cancelled, |readiness| {
        report(step(readiness));
    })?;
    report(cloud::Step::CheckingHerdr);
    let installed =
        install::installed(gateway::prepare(&request, &sandbox.id)?.command, &cancelled)?;
    if !installed {
        if !install {
            return Err(cloud::Error::Install(format!(
                "Herdr is not installed in {}; add it and try again",
                sandbox.name
            ))
            .into());
        }
        report(cloud::Step::InstallingHerdr);
        install::install(gateway::prepare(&request, &sandbox.id)?.command, &cancelled)?;
        if !install::installed(gateway::prepare(&request, &sandbox.id)?.command, &cancelled)? {
            return Err(cloud::Error::Install(
                "the installer finished but Herdr is not on the expected paths".into(),
            )
            .into());
        }
    }
    let device = SavedDevice {
        provider: CloudProvider::Boat,
        id: sandbox.id,
        label: sandbox.name.clone(),
        account: settings.account(),
        machine: sandbox.name,
        session: SESSION.to_owned(),
        enabled: true,
    };
    cloud::save(device.clone())?;
    Ok(device)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    Missing,
    Saved,
    Environment,
}

#[derive(Debug)]
pub(crate) struct Overview {
    pub(crate) key: Key,
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
        Key::Environment
    } else if store::saved(settings.store)?.is_some() {
        Key::Saved
    } else {
        Key::Missing
    };
    let sandboxes = (key != Key::Missing).then(|| {
        let key = store::key(settings)?;
        Request {
            settings,
            key: &key,
        }
        .sandboxes()
        .map(|sandboxes| sandboxes.len())
    });
    Ok(Overview {
        key,
        sandboxes,
        devices,
    })
}

pub(crate) fn save_key(store: Store, key: Option<&SecretString>) -> Result<()> {
    store::save(store, key)
}

pub(crate) fn forget_device(id: &str) -> Result<()> {
    Ok(cloud::remove(CloudProvider::Boat, id)?)
}

#[cfg(test)]
mod tests;
