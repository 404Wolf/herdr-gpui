//! Reaching a saved Azure device. A VM is started if stopped, found by its
//! public IP, and reached through OpenSSH; a sandbox is resumed if suspended
//! and reached through this app's exec stream relay.

use super::{Error, Result, api, gateway::Gateway, http::Client, machine::Machine};
use herdr_client::Transport;
use std::{
    io,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

pub(crate) fn command(account: &str, id: &str, stop: &AtomicBool) -> Result<Command> {
    if let Some(reason) = crate::cloud::unavailable() {
        return Err(crate::cloud::Error::Unavailable(reason).into());
    }

    let config = crate::config::Config::load().map_err(|error| Error::Storage(Box::new(error)))?;

    let settings = config
        .azure
        .settings()
        .map_err(|error| Error::Storage(Box::new(error)))?
        .ok_or(Error::Account)?;

    if settings.account() != account {
        return Err(Error::Account);
    }

    let cancelled = || stop.load(Ordering::Acquire);
    if cancelled() {
        return Err(Error::Cancelled);
    }

    let id = match Machine::parse(id) {
        Machine::Vm(name) => name,
        Machine::Sandbox(sandbox) => {
            return super::sandbox::command(&settings, sandbox, &cancelled);
        }
    };

    let client = Client::new(&settings)?;

    // If VM is deallocated or stopped, start it
    if let Ok(state) = api::get_vm_power_state(&client, id)
        && (state == api::PowerState::Deallocated || state == api::PowerState::Stopped)
    {
        let _ = api::start_vm(&client, id);
    }

    let ip = api::wait_running_ip(&client, id, &cancelled)?;
    super::cleanup::record_activity(id);
    let gateway = Gateway::new(&settings.admin_username, &ip)?;
    gateway.build_command()
}

pub(crate) fn connect(
    account: &str,
    id: &str,
    session: &str,
    stop: &AtomicBool,
) -> io::Result<Transport> {
    let result = command(account, id, stop).and_then(|command| {
        herdr_client::connect_command(command, session, stop).map_err(Error::Bridge)
    });
    match result {
        Ok(bridge) => {
            super::cleanup::record_activity(id);
            Ok(bridge.into())
        }
        Err(Error::Bridge(error)) => Err(io::Error::new(error.kind(), error)),
        Err(error) => Err(io::Error::other(error)),
    }
}
