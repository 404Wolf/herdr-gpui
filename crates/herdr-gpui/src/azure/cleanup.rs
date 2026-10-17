//! Idle tracking and automatic cleanup for Azure Virtual Machines.
//!
//! Tracks last activity times on Azure VM devices. When an Azure device has been
//! idle (or disconnected) for longer than the configured `auto_stop` duration,
//! it can be deallocated to stop compute charges and preserve user credits.

use super::{Result, Settings, api, http::Client};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

static LAST_ACTIVITY: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// Record recent activity on an Azure VM.
pub(crate) fn record_activity(vm_id: &str) {
    if let Ok(mut guard) = LAST_ACTIVITY.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert(vm_id.to_owned(), Instant::now());
    }
}

/// Check if an Azure VM has been idle for at least `timeout`. A VM this
/// process has not seen yet starts its timeout now: the map is empty at
/// launch and a new VM is saved before it connects, so neither counts as
/// idle until a full `timeout` passes without a connection.
pub(crate) fn is_idle(vm_id: &str, timeout: Duration) -> bool {
    let Ok(mut guard) = LAST_ACTIVITY.lock() else {
        return false;
    };
    let last = guard
        .get_or_insert_with(HashMap::new)
        .entry(vm_id.to_owned())
        .or_insert_with(Instant::now);
    last.elapsed() >= timeout
}

/// Reset activity tracking for a VM (e.g. after deallocating or deleting).
pub(crate) fn forget_activity(vm_id: &str) {
    if let Ok(mut guard) = LAST_ACTIVITY.lock()
        && let Some(map) = guard.as_mut()
    {
        map.remove(vm_id);
    }
}

/// Deallocate an Azure VM if it has exceeded its idle timeout.
/// Returns `Ok(true)` if the VM was deallocated, `Ok(false)` if it was active or already stopped.
pub(crate) fn deallocate_if_idle(
    client: &Client,
    vm_name: &str,
    timeout: Duration,
) -> Result<bool> {
    if !is_idle(vm_name, timeout) {
        return Ok(false);
    }

    let power_state = api::get_vm_power_state(client, vm_name)?;
    if power_state == api::PowerState::Running {
        tracing::info!(
            category = "azure_cleanup",
            vm_name,
            timeout_secs = timeout.as_secs(),
            "Deallocating idle Azure VM to prevent ongoing compute charges"
        );
        api::deallocate_vm(client, vm_name)?;
        forget_activity(vm_name);
        return Ok(true);
    }

    Ok(false)
}

/// Clean up an Azure VM and all its associated cloud resources (NIC, disk, IP, VNet, NSG).
#[allow(dead_code)]
pub(crate) fn delete_device_resources(settings: &Settings, vm_name: &str) -> Result<()> {
    forget_activity(vm_name);
    let client = Client::new(settings)?;
    api::delete_vm_and_resources(&client, vm_name)
}

/// The saved Azure VMs of `account`. Sandboxes suspend themselves through
/// their own lifecycle policy, and a device saved under another
/// subscription is not this client's to check: a VM of the same name here
/// would be stopped instead.
fn account_vms(
    devices: Vec<crate::cloud::SavedDevice>,
    account: &str,
) -> Vec<crate::cloud::SavedDevice> {
    devices
        .into_iter()
        .filter(|device| {
            device.provider == crate::cloud::CloudProvider::Azure
                && device.account == account
                && matches!(
                    super::machine::Machine::parse(&device.id),
                    super::machine::Machine::Vm(_)
                )
        })
        .collect()
}

/// Check all saved Azure devices and deallocate any that have been idle for longer than `auto_stop`.
pub(crate) fn check_all_idle_devices() {
    let Ok(devices) = crate::cloud::load() else {
        return;
    };
    if !devices
        .iter()
        .any(|device| device.provider == crate::cloud::CloudProvider::Azure)
    {
        return;
    }

    let Ok(config) = crate::config::Config::load_startup() else {
        return;
    };
    let Ok(Some(settings)) = config.azure.settings() else {
        return;
    };
    let Some(auto_stop) = settings.auto_stop else {
        return;
    };

    let vms = account_vms(devices, &settings.account());
    if vms.is_empty() {
        return;
    }
    let Ok(client) = Client::new(&settings) else {
        return;
    };

    for device in vms {
        if let Err(error) = deallocate_if_idle(&client, &device.id, auto_stop) {
            tracing::warn!(
                category = "azure_cleanup",
                vm_name = %device.id,
                %error,
                "Failed checking or deallocating idle Azure VM"
            );
        }
    }
}

/// Periodically check for idle Azure VMs and deallocate them in the background.
pub(crate) fn install(cx: &mut gpui::App) {
    let timer = cx.background_executor().clone();
    cx.background_executor()
        .spawn(async move {
            loop {
                timer.timer(Duration::from_secs(60)).await;
                check_all_idle_devices();
            }
        })
        .detach();
}

#[cfg(test)]
mod tests;
