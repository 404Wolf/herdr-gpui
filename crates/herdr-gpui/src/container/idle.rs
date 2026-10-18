//! Stopping containers nobody is using, when `[container] auto_stop` asks for
//! it. A container's agents keep working with no window attached, so a
//! closed window says nothing; instead, each minute, the container's own
//! Herdr is asked whether any agent is working or waiting for input. One
//! that has had none for `auto_stop` is stopped: its files stay in its
//! volume, and the next connection starts it again.

use super::{Engine, machine};
use crate::cloud::{CloudProvider, SavedDevice};
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

const CHECK: Duration = Duration::from_secs(60);
const PROBE: Duration = Duration::from_secs(20);

/// Succeeds when an agent there is working or waiting for input. With no
/// server running there is no agent, which the probe reports as idle.
pub(super) const BUSY: &str = r#"PATH="$HOME/.local/bin:$PATH"; herdr agent list 2>/dev/null | grep -Eq '"(working|blocked)"'"#;

/// When each container last had an agent at work, or was first seen running.
static BUSY_AT: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// What to do with one running container.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Keep,
    Stop,
}

/// Decide for the container `id`, given whether an agent is at work there
/// now. A container not seen before starts its timeout now, so one the app
/// has not watched since launch is never stopped at the first check.
pub(super) fn verdict(
    seen: &mut HashMap<String, Instant>,
    id: &str,
    busy: bool,
    now: Instant,
    auto_stop: Duration,
) -> Verdict {
    let since = seen.entry(id.to_owned()).or_insert(now);
    if busy {
        *since = now;
        return Verdict::Keep;
    }
    if now.duration_since(*since) >= auto_stop {
        seen.remove(id);
        return Verdict::Stop;
    }
    Verdict::Keep
}

fn busy(engine: Engine, id: &str) -> bool {
    let probe = [
        "exec",
        "--user",
        machine::USER,
        "--env",
        "HOME=/home/herdr",
        id,
        "/bin/sh",
        "-c",
        BUSY,
    ];
    // A probe that cannot run says nothing; keeping the container is safe.
    !matches!(
        engine.output("exec", probe, PROBE, &|| false),
        Ok((Some(1), ..))
    )
}

fn check(devices: &[SavedDevice], auto_stop: Duration) {
    let now = Instant::now();
    for device in devices {
        let Some(engine) = Engine::from_key(&device.account) else {
            continue;
        };
        if machine::state(engine, &device.id, &|| false).ok() != Some(machine::State::Running) {
            if let Ok(mut seen) = BUSY_AT.lock() {
                seen.get_or_insert_with(HashMap::new).remove(&device.id);
            }
            continue;
        }
        let busy = busy(engine, &device.id);
        let Ok(mut seen) = BUSY_AT.lock() else {
            return;
        };
        let seen = seen.get_or_insert_with(HashMap::new);
        if verdict(seen, &device.id, busy, now, auto_stop) == Verdict::Stop {
            tracing::info!(
                category = "container_idle",
                id = device.id,
                "Stopping a container with no agent at work"
            );
            if let Err(error) = machine::stop(engine, &device.id, &|| false) {
                tracing::warn!(category = "container_idle", id = device.id, %error, "Could not stop an idle container");
            }
        }
    }
}

fn check_all() {
    let Ok(devices) = crate::cloud::load() else {
        return;
    };
    let devices: Vec<_> = devices
        .into_iter()
        .filter(|device| device.provider == CloudProvider::Container)
        .collect();
    if devices.is_empty() {
        return;
    }
    let Ok(config) = crate::config::Config::load_startup() else {
        return;
    };
    let Ok(settings) = config.container.settings() else {
        return;
    };
    if let Some(auto_stop) = settings.auto_stop {
        check(&devices, auto_stop);
    }
}

/// Check containers each minute in the background; see the module docs.
pub(crate) fn install(cx: &mut gpui::App) {
    let timer = cx.background_executor().clone();
    cx.background_executor()
        .spawn(async move {
            loop {
                timer.timer(CHECK).await;
                check_all();
            }
        })
        .detach();
}
