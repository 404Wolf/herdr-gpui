//! Native GitHub transport and device authorization. The one credential
//! subprocess is `gh auth token`, run only to offer the GitHub CLI's sign-in
//! and, once the user chooses it, to read that sign-in again.

mod auth;
mod cli;
mod credentials;
mod device;
mod http;
mod installation;
#[cfg(test)]
pub(crate) use http::fake_graphql;
mod log;
mod routing;
mod store;
mod token;

#[cfg(test)]
mod tests;

pub(crate) use {
    auth::Auth,
    device::{Profile, Source, VERIFY_URL},
    http::{graphql, graphql_aliases, hidden_hint, mutation, rest},
    installation::{Access, Installation},
    routing::Tokens,
    store::{Account, Note, Store, note as store_note, select as select_store},
};

use crate::Result;
use device::{Device, Reply, SETUP_MESSAGE, profile, profile_from, token_reply};
use store::{save, valid_token};

/// Delete one account's saved credential, as signing it out does, for an
/// account whose panel is going away with its device. Blocks on the store:
/// call it from a background thread.
pub(crate) fn forget(store: Store, account: &Account) -> Result<()> {
    save(None, store, account)
}
