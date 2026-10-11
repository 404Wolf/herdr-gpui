//! Reaching a saved Boat sandbox: resume it if stopped, authorize the app's
//! SSH key, pin Boat's authenticated host key, and run Herdr's stdio bridge.

use super::{Error, Result, Settings, api, gateway, http::Request, store};
use herdr_client::Transport;
use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
};

fn connect_sandbox(
    settings: &Settings,
    id: &str,
    session: &str,
    stop: &AtomicBool,
) -> Result<herdr_client::Bridge> {
    let key = store::key(settings)?;
    let request = Request {
        settings,
        key: &key,
    };
    let sandbox = api::wait_ready(
        &request,
        id,
        || stop.load(Ordering::Acquire),
        |readiness| {
            tracing::info!(
                category = "boat_connect",
                ?readiness,
                "Waiting for Boat sandbox"
            );
        },
    )?;
    let gateway = gateway::prepare(&request, &sandbox.id)?;
    herdr_client::connect_command(gateway.command, session, stop).map_err(Error::Bridge)
}

pub(crate) fn connect(
    account: &str,
    id: &str,
    session: &str,
    stop: &AtomicBool,
) -> io::Result<Transport> {
    let result = crate::config::Config::load()
        .map_err(|error| Error::Storage(Box::new(error)))
        .and_then(|config| {
            config
                .boat
                .settings()
                .map_err(|error| Error::Storage(Box::new(error)))
        })
        .and_then(|settings| {
            let settings = settings.ok_or(Error::Account)?;
            if settings.account() != account {
                return Err(Error::Account);
            }
            connect_sandbox(&settings, id, session, stop)
        });
    match result {
        Ok(bridge) => Ok(bridge.into()),
        Err(Error::Bridge(error)) => Err(io::Error::new(error.kind(), error)),
        Err(error) => Err(io::Error::other(error)),
    }
}
