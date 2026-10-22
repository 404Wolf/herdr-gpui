//! `herdr-remote serve`: loads the state directory (certificate authority,
//! devices, VAPID key), connects to the local Herdr daemon, and serves the
//! web app over HTTPS until Herdr UI asks it to stop.

mod control;
mod listener;
mod notifier;
mod options;

pub use crate::push::Privacy;
pub use options::{Certificate, Invocation, Options, ParseError, USAGE, parse};

use crate::{
    Result,
    auth::{Auth, Limits},
    hub::Hub,
    push::{Sender, Vapid},
    tls::{self, Authority, Names, Resolver},
    web::{self, State},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, SystemTime},
};
use tokio::sync::watch;

/// How often the served addresses are checked, so a new network or
/// Tailscale coming up gets a certificate that names it.
const ADDRESS_CHECK: Duration = Duration::from_secs(30);
/// How often idle devices are forgotten.
const IDLE_CHECK: Duration = Duration::from_secs(60 * 60);
/// Who push services may contact about this sender.
const PUSH_SUBJECT: &str = "https://github.com/penso/herdr-gpui";

/// The control endpoint's answer, for other modules' tests.
#[cfg(test)]
pub(crate) fn answer_for_tests(
    state: &State,
    stop: &watch::Sender<bool>,
    request: crate::control::Request,
) -> crate::control::Response {
    control::answer(state, stop, request)
}

/// Runs the daemon on the current Tokio runtime until it is told to stop.
pub async fn run(options: Options) -> Result<()> {
    std::fs::create_dir_all(&options.state_dir)?;
    let names = Names::current();
    let (authority, resolver) = match &options.certificate {
        Certificate::Own => {
            let authority = Authority::load_or_create(&options.state_dir)?;
            let resolver = Resolver::issue(&authority, names.clone(), SystemTime::now())?;
            (Some(authority), resolver)
        }
        Certificate::Files { cert, key } => (None, Resolver::from_files(cert, key)?),
    };
    let fingerprint = match &authority {
        Some(authority) => authority.fingerprint().to_owned(),
        None => colon_hex(&Sha256::digest(resolver.current().cert[0].as_ref())),
    };
    let resolver = Arc::new(resolver);
    let config = Arc::new(tls::server_config(resolver.clone())?);
    let push = Vapid::load_or_create(&options.state_dir)
        .and_then(|vapid| Sender::new(vapid, PUSH_SUBJECT))
        .ok();
    let auth = Auth::load(&options.state_dir, Limits::default())?;
    let hub = Hub::start()?;
    let state = Arc::new(State {
        auth: Mutex::new(auth),
        authority,
        computer: options::computer_name(),
        connected: Mutex::new(HashMap::new()),
        fingerprint,
        hub,
        names: RwLock::new(names),
        port: options.port,
        privacy: options.privacy,
        push,
        uploads: web::Uploads::new(&options.state_dir),
    });
    let (stop, stopped) = watch::channel(false);
    control::serve(&options.state_dir, state.clone(), stop.clone())?;
    tokio::spawn(notifier::run(state.clone()));
    tokio::spawn(watch_addresses(state.clone(), resolver));
    tokio::spawn(expire_idle(state.clone(), options.device_idle));
    let tcp = listener::bind(options.bind, options.port)?;
    let app = web::router(state).into_make_service_with_connect_info::<web::Peer>();
    let mut shutdown = stopped.clone();
    axum::serve(listener::TlsListener::new(tcp, config), app)
        .with_graceful_shutdown(async move {
            let _ = shutdown.wait_for(|stop| *stop).await;
        })
        .await?;
    crate::control::release(&options.state_dir);
    Ok(())
}

/// Reissues the certificate when the computer's addresses change.
async fn watch_addresses(state: Arc<State>, resolver: Arc<Resolver>) {
    let mut ticker = tokio::time::interval(ADDRESS_CHECK);
    loop {
        ticker.tick().await;
        let Ok(names) = tokio::task::spawn_blocking(Names::current).await else {
            continue;
        };
        if let Some(authority) = &state.authority
            && resolver
                .refresh(authority, &names, SystemTime::now())
                .is_err()
        {
            continue;
        }
        if let Ok(mut current) = state.names.write() {
            *current = names;
        }
    }
}

async fn expire_idle(state: Arc<State>, idle: Duration) {
    let idle_ms = u64::try_from(idle.as_millis()).unwrap_or(u64::MAX);
    let mut ticker = tokio::time::interval(IDLE_CHECK);
    loop {
        ticker.tick().await;
        let state = state.clone();
        let _ =
            tokio::task::spawn_blocking(move || state.auth().expire_idle(web::now_ms(), idle_ms))
                .await;
    }
}

/// Uppercase colon-separated hex, as certificate fingerprints read.
fn colon_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
