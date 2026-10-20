//! Read-only, bounded Claude Code transcripts. Call filesystem operations on a worker.
//! This projects an existing agent session; it never launches or owns an agent.
mod parser;
mod store;

pub use parser::parse;
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf};
pub use store::{Reader, resolve};

pub const PAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
pub const MAX_RECORDS: usize = 8192;
pub const MAX_TEXT_BYTES: usize = 32 * 1024;
pub const MAX_PROMPT_BYTES: usize = 20_000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Could not match this pane to a verified Claude session")]
    Identity,
    #[error("No transcript was found for this Claude session")]
    Missing,
    #[error("More than one transcript claims this Claude session")]
    Ambiguous,
    #[error("The transcript is outside Claude's session store")]
    OutsideStore,
    #[error("The transcript exceeds the bounded reader's limits; use Terminal")]
    Limit,
    #[error("The transcript changed while it was being read; try again")]
    Changed,
    #[error("The transcript contains an invalid record")]
    Record(#[source] serde_json::Error),
    #[error("The transcript belongs to another Claude session")]
    WrongSession,
    #[error("Could not read the Claude transcript: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Metadata reported by the daemon's JSON API, not a new binary wire type.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct AgentSession {
    pub agent: String,
    pub kind: SessionKind,
    pub source: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Id,
    Path,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Terminal,
    Chat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Text(String),
    Tool {
        id: String,
        name: String,
        input: String,
        output: Option<String>,
        failed: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub id: String,
    pub role: Role,
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conversation {
    pub turns: Vec<Turn>,
    pub model: Option<String>,
    /// Older bytes/records or oversized individual text blocks were omitted.
    pub limited: bool,
    /// A clear or compaction boundary, when recorded in the loaded history.
    pub boundary: Option<String>,
}

pub fn valid_session_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

fn bounded(text: &str, limited: &mut bool) -> String {
    if text.len() <= MAX_TEXT_BYTES {
        return text.to_owned();
    }
    *limited = true;
    let mut end = MAX_TEXT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[Truncated — use Terminal for full output]",
        &text[..end]
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
