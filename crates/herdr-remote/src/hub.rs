//! Everything the server shows comes through here: the local Herdr daemon's
//! layout ([`daemon`], [`layout`]) and one shared connection per chat pane
//! ([`chat`]). Chats are only ever reached at the address their endpoint id
//! derives, in this user's private directory, never at a path read from
//! daemon data: the same trust boundary as Herdr UI's chat views.

mod chat;
mod daemon;
mod layout;

pub(crate) use chat::{Chat, Event, Summary, View};
pub(crate) use layout::{Chat as ChatRow, Layout, Workspace};

use herdr_chat_protocol::{Agent, Command, EndpointId, Rejection};
use herdr_client::Method;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
};
use tokio::sync::{broadcast, watch};

pub(crate) type Result<T> = std::result::Result<T, Error>;

/// Steps of every chat queued for the push notifier.
const ALL_QUEUE: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("too many requests are waiting")]
    Busy,
    #[error("the chat program cannot be reached")]
    ChatUnavailable,
    #[error("Herdr refused: {0}")]
    Daemon(String),
    #[error("Herdr is not running on this computer")]
    NotConnected,
    #[error("herdr-acp was not found beside herdr-remote or on PATH")]
    ProgramMissing,
    #[error(transparent)]
    Request(#[from] herdr_client::Error),
    #[error("Herdr's answer did not have the expected shape")]
    Response,
    #[error("Herdr did not answer in time")]
    Timeout,
    #[error("no such chat")]
    UnknownChat,
    #[error("no such workspace")]
    UnknownWorkspace,
    // keep-sorted end
}

pub(crate) struct Hub {
    daemon: daemon::Daemon,
    layout: watch::Receiver<Arc<Layout>>,
    chats: Mutex<HashMap<String, (EndpointId, Arc<Chat>)>>,
    all: broadcast::Sender<(String, Event)>,
    program: Option<PathBuf>,
}

impl Hub {
    /// Connects to the local daemon and keeps chat connections in step with
    /// its layout. Must run inside a Tokio runtime.
    pub(crate) fn start() -> std::io::Result<Arc<Self>> {
        let (sender, layout) = watch::channel(Arc::new(Layout::default()));
        let daemon = daemon::Daemon::start(sender)?;
        let (all, _) = broadcast::channel(ALL_QUEUE);
        let hub = Arc::new(Self {
            daemon,
            layout,
            chats: Mutex::new(HashMap::new()),
            all,
            program: locate_program(),
        });
        let follower = hub.clone();
        tokio::spawn(async move { follower.follow().await });
        Ok(hub)
    }

    /// A hub showing `layout` that never connects to a daemon or a chat,
    /// for tests that must not reach a real Herdr.
    #[cfg(test)]
    pub(crate) fn offline(layout: Layout) -> Arc<Self> {
        let (sender, receiver) = watch::channel(Arc::new(layout));
        let (all, _) = broadcast::channel(ALL_QUEUE);
        Arc::new(Self {
            daemon: daemon::Daemon::offline(sender),
            layout: receiver,
            chats: Mutex::new(HashMap::new()),
            all,
            program: None,
        })
    }

    pub(crate) fn layout(&self) -> Arc<Layout> {
        self.layout.borrow().clone()
    }

    pub(crate) fn watch_layout(&self) -> watch::Receiver<Arc<Layout>> {
        self.layout.clone()
    }

    /// Every step of every chat, tagged with its pane.
    pub(crate) fn subscribe_all(&self) -> broadcast::Receiver<(String, Event)> {
        self.all.subscribe()
    }

    pub(crate) fn chat(&self, pane: &str) -> Option<Arc<Chat>> {
        self.chats
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(pane)
            .map(|(_, chat)| chat.clone())
    }

    pub(crate) async fn command(&self, pane: &str, command: Command) -> Result<Option<Rejection>> {
        let chat = self.chat(pane).ok_or(Error::UnknownChat)?;
        chat.command(command).await
    }

    /// Opens a tab in `workspace` running a chat with `agent`, in `cwd` or
    /// the workspace's folder, as Herdr UI's `chat new` does. Returns the
    /// new tab's id.
    pub(crate) async fn start_chat(
        &self,
        workspace: &str,
        agent: Agent,
        cwd: Option<&str>,
    ) -> Result<String> {
        let program = self.program.as_deref().ok_or(Error::ProgramMissing)?;
        let layout = self.layout();
        let folder = layout
            .workspace(workspace)
            .ok_or(Error::UnknownWorkspace)?
            .cwd
            .clone();
        let response = self
            .daemon
            .request(
                Method::TabCreate,
                tab_params(workspace, cwd.unwrap_or(&folder), agent, program),
            )
            .await?;
        let (tab, pane) = created_tab(&response).ok_or(Error::Response)?;
        self.daemon.type_line(&pane, &command_line(agent))?;
        Ok(tab)
    }

    /// A new workspace in `cwd`, on a new worktree when `branch` is given,
    /// with a chat running `agent` in it. Returns the workspace's id.
    pub(crate) async fn create_workspace(
        &self,
        cwd: &str,
        branch: Option<&str>,
        agent: Agent,
    ) -> Result<String> {
        let program = self.program.as_deref().ok_or(Error::ProgramMissing)?;
        let opened = self
            .daemon
            .request(Method::WorkspaceCreate, json!({"cwd": cwd, "focus": false}))
            .await?;
        let workspace = opened["workspace"]["workspace_id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or(Error::Response)?
            .to_owned();
        let Some(branch) = branch else {
            let response = self
                .daemon
                .request(
                    Method::TabCreate,
                    tab_params(&workspace, cwd, agent, program),
                )
                .await?;
            let (_, pane) = created_tab(&response).ok_or(Error::Response)?;
            self.daemon.type_line(&pane, &command_line(agent))?;
            return Ok(workspace);
        };
        let created = self
            .daemon
            .request(
                Method::WorktreeCreate,
                json!({"workspace_id": workspace, "branch": branch, "focus": false}),
            )
            .await?;
        let pane = created["root_pane"]["pane_id"]
            .as_str()
            .ok_or(Error::Response)?;
        let worktree = created["workspace"]["workspace_id"]
            .as_str()
            .ok_or(Error::Response)?
            .to_owned();
        // The worktree's first pane is a shell in its folder; the program
        // path rides in this line because the pane was not made with it.
        self.daemon
            .type_line(pane, &direct_command_line(program, agent))?;
        Ok(worktree)
    }

    /// Starts connections for new chat panes and closes those whose pane
    /// went away or whose program changed.
    async fn follow(self: Arc<Self>) {
        let mut layout = self.layout.clone();
        loop {
            let current = layout.borrow_and_update().clone();
            self.reconcile(&current);
            if layout.changed().await.is_err() {
                return;
            }
        }
    }

    fn reconcile(&self, layout: &Layout) {
        let mut chats = self.chats.lock().unwrap_or_else(PoisonError::into_inner);
        chats.retain(|pane, (endpoint, chat)| {
            let keep = layout
                .chat(pane)
                .is_some_and(|(_, current)| &current.endpoint == endpoint);
            if !keep {
                chat.close();
            }
            keep
        });
        for chat in layout.chats() {
            if chats.contains_key(&chat.pane) {
                continue;
            }
            let Some(address) = herdr_chat_protocol::endpoint_address(&chat.endpoint) else {
                continue;
            };
            if let Ok(link) = Chat::start(chat.pane.clone(), address, self.all.clone()) {
                chats.insert(chat.pane.clone(), (chat.endpoint.clone(), link));
            }
        }
    }
}

/// The environment variable the typed line runs the program from, as in
/// Herdr UI.
const PROGRAM_ENV: &str = "HERDR_ACP";
const PROGRAM: &str = if cfg!(windows) {
    "herdr-acp.exe"
} else {
    "herdr-acp"
};

/// The `herdr-acp` beside this executable (the app bundle ships both),
/// else the first on `PATH`.
fn locate_program() -> Option<PathBuf> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(PROGRAM)));
    beside
        .into_iter()
        .chain(
            std::env::var_os("PATH")
                .into_iter()
                .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
                .filter(|dir| dir.is_absolute())
                .map(|dir| dir.join(PROGRAM)),
        )
        .find(|candidate| candidate.is_file())
}

fn tab_params(workspace: &str, cwd: &str, agent: Agent, program: &Path) -> Value {
    json!({
        "workspace_id": workspace,
        "label": agent.display_name(),
        "focus": false,
        "cwd": cwd,
        "env": { PROGRAM_ENV: program.to_string_lossy() },
    })
}

/// The tab and root pane a `tab_created` answer names.
fn created_tab(result: &Value) -> Option<(String, String)> {
    let id = |value: &Value| {
        value
            .as_str()
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
    };
    (result["type"] == "tab_created").then_some(())?;
    Some((
        id(&result["tab"]["tab_id"])?,
        id(&result["root_pane"]["pane_id"])?,
    ))
}

/// The line typed into a new chat tab, the same as Herdr UI types: the
/// program comes from the tab's environment, so nothing typed depends on
/// where it is installed.
fn command_line(agent: Agent) -> String {
    if cfg!(windows) {
        format!("& $env:{PROGRAM_ENV} {agent}")
    } else {
        format!(" sh -c 'exec \"${PROGRAM_ENV}\" {agent}'")
    }
}

/// The line for a pane made without the program in its environment.
fn direct_command_line(program: &Path, agent: Agent) -> String {
    let program = program.to_string_lossy();
    if cfg!(windows) {
        format!("& '{}' {agent}", program.replace('\'', "''"))
    } else {
        format!(" {} {agent}", herdr_client::shell_quote(&program))
    }
}

/// A terminal row, for other modules' tests.
#[cfg(test)]
pub(crate) fn layout_terminal(tab: &str, label: &str) -> layout::Terminal {
    layout::Terminal {
        tab: tab.into(),
        label: label.into(),
        cwd: None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
