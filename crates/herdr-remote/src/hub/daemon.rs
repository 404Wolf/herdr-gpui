//! The connection to this machine's Herdr daemon: an inactive client (no
//! surface interest, so it never sizes a pane) whose snapshots become the
//! [`Layout`] phones list, and whose endpoint requests create workspaces
//! and tabs. A worker thread owns the blocking I/O; the server only reads
//! the layout and awaits responses.

use super::{
    Error, Result,
    layout::{self, Layout},
};
use herdr_client::{
    ClientEvent, ClientHandle, ConnectOptions, ConnectTarget, Method,
    protocol::{ClientKeyCode, ClientKeyKind, ClientPaneInputEvent},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use tokio::sync::{oneshot, watch};

/// How long the worker waits before connecting again.
const RETRY: Duration = Duration::from_secs(3);
/// How long a request waits for the daemon's answer.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
/// Requests waiting for an answer at once.
const MAX_PENDING: usize = 16;

pub(crate) struct Daemon {
    shared: Arc<Shared>,
}

struct Shared {
    /// The live connection's handle and the boot its snapshot came from.
    handle: Mutex<Option<(ClientHandle, String)>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    layout: watch::Sender<Arc<Layout>>,
    stop: AtomicBool,
}

impl Daemon {
    /// Starts the worker, which connects now and again whenever the daemon
    /// goes away.
    pub(crate) fn start(
        layout: watch::Sender<Arc<Layout>>,
        target: ConnectTarget,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            handle: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            layout,
            stop: AtomicBool::new(false),
        });
        let worker = shared.clone();
        thread::Builder::new()
            .name("herdr-remote-daemon".into())
            .spawn(move || run(&worker, &target))?;
        Ok(Self { shared })
    }

    /// A daemon that never connects, for tests that must not reach a real
    /// Herdr.
    #[cfg(test)]
    pub(crate) fn offline(layout: watch::Sender<Arc<Layout>>) -> Self {
        Self {
            shared: Arc::new(Shared {
                handle: Mutex::new(None),
                pending: Mutex::new(HashMap::new()),
                layout,
                stop: AtomicBool::new(true),
            }),
        }
    }

    /// Sends an endpoint request on the current boot and waits for its
    /// `result`.
    pub(crate) async fn request(&self, method: Method, params: Value) -> Result<Value> {
        let (sender, receiver) = oneshot::channel();
        let id = {
            let handle = lock(&self.shared.handle);
            let (handle, boot) = handle.as_ref().ok_or(Error::NotConnected)?;
            let mut pending = lock(&self.shared.pending);
            if pending.len() >= MAX_PENDING {
                return Err(Error::Busy);
            }
            let id = handle.request(boot, method, params)?;
            pending.insert(id.clone(), sender);
            id
        };
        // A request that times out or whose caller goes away gives its slot
        // back; one answered was already taken out.
        let _slot = Slot {
            pending: &self.shared.pending,
            id,
        };
        let response = tokio::time::timeout(RESPONSE_TIMEOUT, receiver)
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::NotConnected)?;
        result(response)
    }

    /// Types `line` and Enter into `pane`, as Herdr UI starts a chat.
    pub(crate) fn type_line(&self, pane: &str, line: &str) -> Result<()> {
        let handle = lock(&self.shared.handle);
        let (handle, boot) = handle.as_ref().ok_or(Error::NotConnected)?;
        handle.send_input(
            boot,
            pane,
            [ClientPaneInputEvent::TextCommit(line.into()), enter()],
        )?;
        Ok(())
    }
}

/// Removes a request from the pending table when dropped.
struct Slot<'a> {
    pending: &'a Mutex<HashMap<String, oneshot::Sender<Value>>>,
    id: String,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        lock(self.pending).remove(&self.id);
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some((handle, _)) = lock(&self.shared.handle).take() {
            handle.disconnect();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The `result` of an endpoint response, or its error.
pub(crate) fn result(response: Value) -> Result<Value> {
    match response.get("error").filter(|error| !error.is_null()) {
        Some(error) => Err(Error::Daemon(
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("the request failed")
                .chars()
                .take(200)
                .collect(),
        )),
        None => Ok(response.get("result").cloned().unwrap_or(Value::Null)),
    }
}

fn enter() -> ClientPaneInputEvent {
    ClientPaneInputEvent::Key {
        code: ClientKeyCode::Enter,
        modifiers: 0,
        kind: ClientKeyKind::Press,
        repeat_count: 1,
        shifted_codepoint: None,
        generated_text: None,
        tracks_release: false,
        physical_key_id: None,
        windows_record: None,
    }
}

fn run(shared: &Shared, target: &ConnectTarget) {
    while !shared.stop.load(Ordering::Acquire) {
        if let Ok(client) = herdr_client::connect_with_surface_active(
            target.clone(),
            ConnectOptions::default(),
            false,
        ) {
            serve(shared, client);
        }
        *lock(&shared.handle) = None;
        // Requests in flight end with the connection.
        lock(&shared.pending).clear();
        shared.layout.send_replace(Arc::new(Layout::default()));
        thread::sleep(RETRY);
    }
}

fn serve(shared: &Shared, client: herdr_client::Client) {
    let handle = client.handle.clone();
    while let Ok(event) = client.events.recv() {
        if shared.stop.load(Ordering::Acquire) {
            handle.disconnect();
            return;
        }
        match event {
            ClientEvent::Snapshot(snapshot) => {
                *lock(&shared.handle) = Some((handle.clone(), snapshot.boot_id.clone()));
                let next = layout::project(&snapshot);
                shared.layout.send_if_modified(|current| {
                    let changed = **current != next;
                    if changed {
                        *current = Arc::new(next);
                    }
                    changed
                });
            }
            ClientEvent::Response {
                request_id,
                response,
            } => {
                if let Some(sender) = lock(&shared.pending).remove(&request_id) {
                    let _ = sender.send(response);
                }
            }
            ClientEvent::CommandRejected {
                request_id: Some(request_id),
                reason,
            } => {
                if let Some(sender) = lock(&shared.pending).remove(&request_id) {
                    let message = reason.to_string();
                    let _ = sender.send(serde_json::json!({"error": {"message": message}}));
                }
            }
            ClientEvent::Disconnected { .. } => return,
            _ => {}
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
