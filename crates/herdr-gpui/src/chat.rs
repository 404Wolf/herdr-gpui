//! A native view of a `herdr-acp` chat, drawn over the pane the program runs
//! in. The program is the agent's only ACP client and draws its own terminal
//! view; it also listens on a private endpoint (a socket on Unix, a named
//! pipe on Windows), and this window attaches there as one more client of
//! the same conversation. Both views stay
//! interactive: a prompt, a cancel, or a permission answer may come from
//! either, and the first answer to a request wins.
//!
//! The pane names its program through a metadata token holding an
//! [`EndpointId`]; the address is derived from it (in this user's own state
//! directory on Unix), never read from daemon data, and only for a daemon on
//! this machine that this user owns. Each attached pane has a worker
//! ([`link`]) that does the blocking I/O, a [`session`] that applies what it
//! reads, and a view drawn from prepared state. A pane whose endpoint cannot
//! be reached shows its terminal cells as before.

mod chats;
mod discovery;
mod link;
mod list;
mod prepared;
mod session;
mod start;
mod view;
mod window;

pub(crate) use chats::Chats;

/// The wall clock in milliseconds since the Unix epoch, which the program
/// on this machine stamps a running turn with.
pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}
pub(crate) use start::{Error as StartError, Program};

use herdr_chat_protocol::{ByeReason, EndpointId};
use std::io;

/// Chat panes attached at once; more are left to their terminal views.
const MAX_CHATS: usize = 16;
/// Commands waiting for their acknowledgement, per pane.
const MAX_PENDING: usize = 32;
/// Messages a worker holds for the window before it gives the connection up
/// as too far behind; the program then replays from the cursor.
const MAX_EVENTS: usize = 1024;
/// Connection failures in a row after which the pane shows its terminal.
const MAX_FAILURES: u32 = 3;

pub(crate) type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    /// A change that does not fit the transcript this client holds.
    #[error("a chat change does not fit the transcript")]
    Apply(#[source] herdr_chat_protocol::Error),
    #[error("the chat program left: {0:?}")]
    Bye(ByeReason),
    #[error("too many chat commands are waiting for an answer")]
    Busy,
    #[error("the chat program closed the connection")]
    Closed,
    #[error("could not reach the chat program")]
    Connect(#[source] io::Error),
    #[error("the chat program did not greet first")]
    Handshake,
    #[error("no private directory holds chat sockets")]
    NoEndpointDir,
    #[error("the chat view is not attached")]
    NotAttached,
    #[error("the chat view fell behind the program")]
    Overflow,
    #[error("could not read from the chat program")]
    Read(#[source] herdr_chat_protocol::Error),
    #[error("could not start the chat worker")]
    Spawn(#[source] io::Error),
    #[error(transparent)]
    Start(#[from] start::Error),
    #[error("chat change {got:?} does not follow {after:?}")]
    Stream {
        after: Option<herdr_chat_protocol::Cursor>,
        got: herdr_chat_protocol::Cursor,
    },
    #[error("the chat program speaks protocol version {0}")]
    Version(u32),
    // keep-sorted end
}

impl Error {
    /// Whether trying again cannot help, so the pane keeps its terminal.
    fn permanent(&self) -> bool {
        matches!(
            self,
            Self::Version(_) | Self::Bye(ByeReason::Version { .. }) | Self::NoEndpointDir
        )
    }

    /// Whether the client's copy no longer fits the program's stream, so the
    /// next connection asks for a whole snapshot.
    fn desynced(&self) -> bool {
        matches!(self, Self::Stream { .. } | Self::Apply(_))
    }
}

/// The pane a chat belongs to, and the program it talks to.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct Target {
    pub(crate) endpoint: EndpointId,
    pub(crate) pane_id: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
