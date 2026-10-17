//! Destroying a saved Azure device: the VM or sandbox goes at Azure with
//! everything it bills for, its disk and public IP included, and then the
//! device leaves the catalog. Devices are meant to be temporary, so nothing
//! is kept for a later restore.

use super::{Error, Result, Settings, api, http::Client, machine::Machine};
use crate::cloud::{self, CloudProvider, SavedDevice};

/// What a destroy is waiting on, for its progress bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    Vm,
    /// The VM is gone; its network and any disk left by an older VM go now.
    Leftovers,
    Sandbox,
}

/// Delete `device` at Azure and return once Azure no longer has it, then
/// forget it here. Blocks: run it on a worker. A cancelled destroy leaves
/// the device saved, so running Destroy again finishes the job.
pub(crate) fn destroy_device(
    settings: &Settings,
    device: &SavedDevice,
    cancelled: &dyn Fn() -> bool,
    mut stage: impl FnMut(Stage),
) -> Result<()> {
    if device.provider != CloudProvider::Azure || device.account != settings.account() {
        return Err(Error::Account);
    }
    match Machine::parse(&device.id) {
        Machine::Vm(name) => {
            stage(Stage::Vm);
            let client = Client::new(settings)?;
            api::delete_vm(&client, name, cancelled, || stage(Stage::Leftovers))?;
        }
        Machine::Sandbox(sandbox) => {
            stage(Stage::Sandbox);
            super::sandbox::delete(settings, sandbox, cancelled)?;
        }
    }
    super::cleanup::forget_activity(&device.id);
    Ok(cloud::remove(CloudProvider::Azure, &device.id)?)
}

/// Destroy the saved device `id` with the configured account, as the
/// sidebar's Remove does when asked to delete it at Azure too. A device
/// already gone from the catalog has nothing left to do. Blocks: run it off
/// the UI thread.
pub(crate) fn destroy_saved(id: &str) -> crate::Result<()> {
    let config = crate::config::Config::load()?;
    let settings = config.azure.settings()?.ok_or(Error::Account)?;
    let Some(device) = cloud::load()?
        .into_iter()
        .find(|device| device.provider == CloudProvider::Azure && device.id == id)
    else {
        return Ok(());
    };
    Ok(destroy_device(&settings, &device, &|| false, |_| {})?)
}
