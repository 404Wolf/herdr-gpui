//! One bounded worker per opened chat. Reads and submits are serialized; closing
//! the view cancels owned queries without joining a worker on the UI thread.
use super::{Error, Result};
use crate::teleport::{Envelope, Host};
use herdr_client::{
    ClientHandle, ConnectTarget,
    chat::{self, AgentSession, Conversation, SessionKind},
};
use serde::Deserialize;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

const DEADLINE: Duration = Duration::from_secs(5);

pub(super) fn supports(target: &ConnectTarget) -> bool {
    matches!(
        target,
        ConnectTarget::Local
            | ConnectTarget::Session {
                development: false,
                ..
            }
    ) && matches!((target.socket_path(), target.local_session_socket_path()), (Ok(actual), Ok(standard)) if actual == standard)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Identity {
    pub terminal: String,
    pub session: AgentSession,
    pub directory: PathBuf,
    pub runtime: Option<super::session::Runtime>,
}

#[derive(Deserialize)]
struct AgentResult {
    agent: Agent,
}
#[derive(Deserialize)]
struct Agent {
    pane_id: String,
    terminal_id: String,
    agent: Option<String>,
    agent_status: herdr_client::protocol::AgentStatus,
    agent_session: Option<AgentSession>,
    #[serde(skip)]
    directory: PathBuf,
    #[serde(skip)]
    runtime: Option<super::session::Runtime>,
}

pub(super) struct Snapshot {
    pub identity: Identity,
    pub conversation: Arc<Conversation>,
    pub rows: Arc<[super::rows::Row]>,
}
pub(super) enum Request {
    Read { pages: usize },
    Send { identity: Identity, text: String },
}
pub(super) enum Reply {
    Read(Result<Snapshot>),
    Sent(Result<()>),
}

pub(super) struct Worker {
    tx: mpsc::SyncSender<Request>,
    pub rx: mpsc::Receiver<Reply>,
    cancelled: Arc<AtomicBool>,
}

impl Worker {
    pub fn start(target: &ConnectTarget, pane: String, handle: ClientHandle) -> Result<Self> {
        if !supports(target) {
            return Err(Error::Unsupported);
        }
        let host = Host::new(target).map_err(|_| Error::Unsupported)?;
        let (tx, requests) = mpsc::sync_channel(1);
        let (replies, rx) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        std::thread::Builder::new()
            .name("claude-chat".into())
            .spawn(move || {
                let mut reader = chat::Reader::default();
                let mut prepared: Option<(Arc<Conversation>, Arc<[super::rows::Row]>)> = None;
                while let Ok(request) = requests.recv() {
                    if stop.load(Ordering::Acquire) || handle.is_disconnected() {
                        break;
                    }
                    let reply = match request {
                        Request::Read { pages } => Reply::Read((|| {
                            let identity = identity(&query(&host, &pane, &stop)?, &pane)?;
                            let conversation =
                                match chat::resolve(&identity.directory, &identity.session.value) {
                                    Ok(path) => {
                                        reader.read(&path, &identity.session.value, pages)?
                                    }
                                    Err(chat::Error::Missing) => Arc::new(Conversation::default()),
                                    Err(error) => return Err(error.into()),
                                };
                            if self_same(&host, &pane, &identity, &stop)? {
                                let rows = prepared
                                    .as_ref()
                                    .filter(|(old, _)| Arc::ptr_eq(old, &conversation))
                                    .map(|(_, rows)| rows.clone())
                                    .unwrap_or_else(|| super::rows::prepare(&conversation));
                                prepared = Some((conversation.clone(), rows.clone()));
                                Ok(Snapshot {
                                    identity,
                                    conversation,
                                    rows,
                                })
                            } else {
                                Err(Error::TargetChanged)
                            }
                        })()),
                        Request::Send {
                            identity: expected,
                            text,
                        } => Reply::Sent(send(&host, &pane, &expected, &text, &stop, || {
                            !handle.is_disconnected()
                        })),
                    };
                    if replies.send(reply).is_err() {
                        break;
                    }
                }
            })
            .map_err(Error::Thread)?;
        Ok(Self { tx, rx, cancelled })
    }

    pub fn request(&self, request: Request) -> Result<()> {
        self.tx.try_send(request).map_err(|_| Error::Stopped)
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

fn query(host: &Host, pane: &str, stop: &AtomicBool) -> Result<Agent> {
    let output = host
        .herdr_exec(&["agent", "get", pane], DEADLINE, stop)
        .map_err(Error::Query)?;
    let mut agent = serde_json::from_slice::<Envelope<AgentResult>>(&output)
        .map(|answer| answer.result.agent)
        .map_err(Error::Decode)?;
    if agent.pane_id != pane
        || agent.agent.as_deref() != Some("claude")
        || agent.terminal_id.is_empty()
    {
        return Err(Error::TargetChanged);
    }
    let resolved = super::session::resolve(host, pane, agent.agent_session.as_ref(), stop)?;
    agent.agent_session = Some(resolved.session);
    agent.directory = resolved.directory;
    agent.runtime = resolved.runtime;
    Ok(agent)
}

fn identity(agent: &Agent, pane: &str) -> Result<Identity> {
    let session = agent.agent_session.as_ref().filter(|s| {
        s.agent == "claude" && s.kind == SessionKind::Id && chat::valid_session_id(&s.value)
    });
    if agent.pane_id != pane
        || agent.agent.as_deref() != Some("claude")
        || agent.terminal_id.is_empty()
    {
        return Err(Error::TargetChanged);
    }
    Ok(Identity {
        terminal: agent.terminal_id.clone(),
        session: session.cloned().ok_or(chat::Error::Identity)?,
        directory: agent.directory.clone(),
        runtime: agent.runtime.clone(),
    })
}

fn self_same(host: &Host, pane: &str, expected: &Identity, stop: &AtomicBool) -> Result<bool> {
    Ok(identity(&query(host, pane, stop)?, pane)? == *expected)
}

fn send(
    host: &Host,
    pane: &str,
    expected: &Identity,
    text: &str,
    stop: &AtomicBool,
    connected: impl Fn() -> bool,
) -> Result<()> {
    checked_send(
        pane,
        expected,
        text,
        || query(host, pane, stop),
        || super::input_guard::screen(host, pane, &expected.terminal, stop),
        || {
            host.herdr_exec(&["agent", "prompt", pane, text], DEADLINE, stop)
                .map(drop)
                .map_err(Error::Unconfirmed)
        },
        || !stop.load(Ordering::Acquire) && connected(),
    )
}

fn checked_send(
    pane: &str,
    expected: &Identity,
    text: &str,
    mut query: impl FnMut() -> Result<Agent>,
    read: impl FnOnce() -> Result<Vec<u8>>,
    submit: impl FnOnce() -> Result<()>,
    current: impl Fn() -> bool,
) -> Result<()> {
    validate_prompt(text)?;
    let check = |agent: Agent| {
        use herdr_client::protocol::AgentStatus;
        if !current() || identity(&agent, pane)? != *expected {
            return Err(Error::TargetChanged);
        }
        if !matches!(agent.agent_status, AgentStatus::Idle | AgentStatus::Done) {
            return Err(Error::NotReady);
        }
        Ok(())
    };
    check(query()?)?;
    // Detection reads are the live screen, not a user-scrolled viewport.
    let screen = read()?;
    if !empty_input(&String::from_utf8_lossy(&screen)) {
        return Err(Error::InputOccupied);
    }
    check(query()?)?;
    // agent.prompt owns its paste+Enter transaction. Never retry an uncertain write.
    submit()
}

fn validate_prompt(text: &str) -> Result<()> {
    if text.trim().is_empty()
        || text.len() > chat::MAX_PROMPT_BYTES
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
    {
        Err(Error::InvalidPrompt)
    } else {
        Ok(())
    }
}

/// Positive proof of a plain empty prompt between two horizontal rules.
/// Unknown layouts and unstyled suggestion text stay in Terminal.
fn empty_input(screen: &str) -> bool {
    super::input_guard::empty(screen)
}

#[cfg(test)]
mod tests;
