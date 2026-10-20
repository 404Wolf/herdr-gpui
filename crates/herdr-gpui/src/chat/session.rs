//! Session binding when Claude was launched without Herdr's session hook.
//! Native PID records are evidence only after the process start identity is
//! checked. Neither cwd nor transcript modification time chooses a session.
use super::{Error, Result};
use crate::teleport::Host;
use herdr_client::chat::{self, AgentSession, SessionKind};
use std::{path::PathBuf, sync::atomic::AtomicBool};

#[cfg(any(test, target_os = "macos", target_os = "linux"))]
mod native;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Runtime {
    pub pid: u32,
    pub started: String,
}

pub(super) struct Resolved {
    pub session: AgentSession,
    pub directory: PathBuf,
    pub runtime: Option<Runtime>,
}

pub(super) fn resolve(
    host: &Host,
    pane: &str,
    reported: Option<&AgentSession>,
    stop: &AtomicBool,
) -> Result<Resolved> {
    let directory = crate::login_env::var("CLAUDE_CONFIG_DIR")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::config::home().ok().map(|home| home.join(".claude")))
        .filter(|path| path.is_absolute())
        .ok_or(Error::Config)?;
    if let Some(session) = reported.filter(|s| {
        s.agent == "claude" && s.kind == SessionKind::Id && chat::valid_session_id(&s.value)
    }) {
        return Ok(Resolved {
            session: session.clone(),
            directory,
            runtime: None,
        });
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    return native::resolve(host, pane, &directory, stop);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (host, pane, stop);
        Err(chat::Error::Identity.into())
    }
}
