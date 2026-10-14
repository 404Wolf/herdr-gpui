//! Browser tabs for agents on the hosts a window connects to. An agent on an
//! SSH host has no path to this machine's control socket, and Herdr has no
//! message that asks a client to show a page, so a request travels as
//! metadata tokens on the caller's workspace: the managed `herdr-gpui` shim
//! this app writes on the host ([`install`]) sets them with `herdr workspace
//! report-metadata` and a short time to live, and every window connected to
//! that daemon sees them in its snapshot. Each request carries an ID, and the
//! app acts on an ID once, in whichever window sees it first.
//!
//! Tokens are daemon data that any process on the host may set, so a request
//! can do no more than open an http or https page in the workspace that
//! carries it, or reload the tabs its pane opened. A page on the host's own
//! loopback is reached only through a forward this window starts, as
//! `ports open` does.

mod install;
mod window;

pub(crate) use window::Forwarding;

use crate::browser::Scope;
use gpui::Global;
use std::collections::VecDeque;

/// Every token of a request starts with this, so other plugins' tokens and
/// lists of reported values can tell them apart.
const PREFIX: &str = "herdr-gpui-";
const ID: &str = "herdr-gpui-id";
const PANE: &str = "herdr-gpui-pane";
const ACTION: &str = "herdr-gpui-do";
/// The address, cut into numbered tokens of at most 80 characters, the most
/// Herdr keeps in one value.
const URL: &str = "herdr-gpui-url-";
const URL_CHUNKS: usize = 12;
/// Identifiers are echoed in no answer, but kept to Herdr's own token bound.
const MAX_ID: usize = 80;

/// Whether `name` is one of a request's tokens.
pub(crate) fn is_request_token(name: &str) -> bool {
    name.starts_with(PREFIX)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    /// Open the address, as the agent typed it, and switch to the tab unless
    /// `focus` is false.
    Open { address: String, focus: bool },
    /// Reload the tabs the requesting pane opened.
    Reload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub id: String,
    /// The pane of the agent asking, which notes on its pages go back to.
    pub pane: Option<String>,
    pub action: Action,
}

fn plain(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ID
        && value.chars().all(|c| !c.is_control() && !c.is_whitespace())
}

impl Request {
    /// The request a workspace's tokens carry, if a whole one is there.
    pub(crate) fn from_tokens(tokens: &[(String, String)]) -> Option<Self> {
        let value = |key: &str| {
            tokens
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str())
        };
        let id = value(ID).filter(|id| plain(id))?.to_owned();
        let pane = value(PANE).filter(|pane| plain(pane)).map(str::to_owned);
        let action = match value(ACTION)? {
            action @ ("open" | "open-background") => {
                // Numbered from 0; the first missing number ends the address.
                let address: String = (0..URL_CHUNKS)
                    .map_while(|index| value(&format!("{URL}{index}")))
                    .collect();
                if address.is_empty() {
                    return None;
                }
                Action::Open {
                    address,
                    focus: action == "open",
                }
            }
            "reload" => Action::Reload,
            _ => return None,
        };
        Some(Self { id, pane, action })
    }
}

/// Requests already acted on, so a request is served once although its
/// tokens stay in every snapshot until they expire.
#[derive(Default)]
pub(crate) struct Seen(VecDeque<SeenKey>);

/// A request by its daemon, workspace, and ID. Reloads are per window, since
/// each window has its own pages; opens are the app's, so `window` is `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SeenKey {
    pub scope: Scope,
    pub workspace: String,
    pub id: String,
    pub window: Option<u64>,
}

/// Far more than the requests a few hosts make within a token's lifetime.
const MAX_SEEN: usize = 256;

impl Global for Seen {}

impl Seen {
    /// Whether `key` is new, remembering it.
    pub(crate) fn first(&mut self, key: SeenKey) -> bool {
        if self.0.contains(&key) {
            return false;
        }
        if self.0.len() >= MAX_SEEN {
            self.0.pop_front();
        }
        self.0.push_back(key);
        true
    }
}

#[cfg(test)]
mod tests;
