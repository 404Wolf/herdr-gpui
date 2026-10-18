//! Agents that left a host while it stayed up: their pane or tab closed, or
//! the agent exited. The daemon forgets them at once, so the page keeps the
//! last row it drew of each, with the folder it ran in, to list under Closed
//! and to start again (`reopen`). The window keeps them while it is open. A
//! daemon restart is not a close: a new boot id starts the host afresh.

use super::model::{Key, Open, Row};
use herdr_client::protocol::ClientShellSnapshot;
use std::{collections::HashMap, time::SystemTime};

/// Closed agents kept; the oldest go first beyond this.
const MAX_CLOSED: usize = 32;

/// An agent that closed, as it last showed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Agent {
    pub row: Row,
    /// The folder its pane last ran in, for the tab that reopens it.
    pub cwd: Option<String>,
    pub closed: SystemTime,
}

/// One host's agents as its last report listed them.
#[derive(Debug)]
struct Seen {
    boot: String,
    agents: Vec<(Row, Option<String>)>,
}

#[derive(Debug, Default)]
pub(crate) struct Closed {
    /// By endpoint id.
    seen: HashMap<String, Seen>,
    /// Oldest first.
    agents: Vec<Agent>,
    /// Counts changes, so the page redraws when one closes or goes.
    revision: u64,
}

/// The pane an agent row stands in.
fn pane(row: &Row) -> Option<&str> {
    match &row.open {
        Open::Pane { pane, .. } => Some(pane),
        Open::Run(_) => None,
    }
}

/// The folder a row's pane runs in: its foreground process's, else the
/// shell's.
fn cwd(snapshot: &ClientShellSnapshot, row: &Row) -> Option<String> {
    let pane = snapshot
        .panes
        .iter()
        .find(|shown| Some(shown.pane_id.as_str()) == pane(row))?;
    [&pane.foreground_cwd, &pane.cwd]
        .into_iter()
        .flatten()
        .find(|path| !path.is_empty())
        .cloned()
}

impl Closed {
    /// Notes `endpoint`'s agents from its report `snapshot`: those listed on
    /// the last report of the same boot and gone from this one closed. An
    /// agent that turns up again, reopened or started anew, is no longer
    /// listed as closed.
    pub(crate) fn observe<'a>(
        &mut self,
        endpoint: &str,
        snapshot: &ClientShellSnapshot,
        rows: impl IntoIterator<Item = &'a Row>,
        now: SystemTime,
    ) {
        let current: Vec<(Row, Option<String>)> = rows
            .into_iter()
            .filter(|row| row.endpoint == endpoint && pane(row).is_some())
            .map(|row| (row.clone(), cwd(snapshot, row)))
            .collect();
        let seen = Seen {
            boot: snapshot.boot_id.clone(),
            agents: current,
        };
        let previous = self
            .seen
            .insert(endpoint.to_owned(), seen)
            .filter(|previous| previous.boot == snapshot.boot_id)
            .map(|previous| previous.agents)
            .unwrap_or_default();
        let Some(current) = self.seen.get(endpoint).map(|seen| &seen.agents) else {
            return;
        };
        let before = self.agents.len();
        // Two agents of one name in one checkout share a key, so only a key
        // new on this report brings a closed agent back.
        self.agents.retain(|agent| {
            agent.row.endpoint != endpoint
                || !current.iter().any(|(row, _)| row.key == agent.row.key)
                || previous.iter().any(|(row, _)| row.key == agent.row.key)
        });
        let mut changed = self.agents.len() != before;
        for (row, cwd) in previous {
            if current.iter().any(|(shown, _)| pane(shown) == pane(&row)) {
                continue;
            }
            self.agents.retain(|agent| agent.row.key != row.key);
            if self.agents.len() >= MAX_CLOSED {
                self.agents.remove(0);
            }
            self.agents.push(Agent {
                row,
                cwd,
                closed: now,
            });
            changed = true;
        }
        if changed {
            self.revision += 1;
        }
    }

    /// Forgets hosts the window no longer has, and their closed agents.
    pub(crate) fn retain_hosts(&mut self, mut kept: impl FnMut(&str) -> bool) {
        self.seen.retain(|endpoint, _| kept(endpoint));
        let before = self.agents.len();
        self.agents.retain(|agent| kept(&agent.row.endpoint));
        if self.agents.len() != before {
            self.revision += 1;
        }
    }

    /// Newest first, as the page lists them.
    pub(crate) fn agents(&self) -> impl Iterator<Item = &Agent> {
        self.agents.iter().rev()
    }

    pub(crate) fn len(&self) -> usize {
        self.agents.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    pub(crate) fn get(&self, key: &Key) -> Option<&Agent> {
        self.agents.iter().find(|agent| agent.row.key == *key)
    }

    pub(crate) fn forget(&mut self, key: &Key) {
        let before = self.agents.len();
        self.agents.retain(|agent| agent.row.key != *key);
        if self.agents.len() != before {
            self.revision += 1;
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
}
