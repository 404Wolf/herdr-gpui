//! Which signed-in account reads a repository: an additional account when the
//! repository's owner is its login, one of its organizations, or an owner its
//! config lists; otherwise the main sign-in. Workers learn a checkout's owner
//! from its remote, so they carry every token and choose once they know it.

use super::Profile;
use secrecy::SecretString;
use std::sync::Arc;

/// One account's token and the owners it reads; the main account's list is
/// empty because it reads every owner no other account claims.
struct Route {
    login: String,
    token: Arc<SecretString>,
    owners: Vec<String>,
}

impl Route {
    fn covers(&self, owner: &str) -> bool {
        self.owners
            .iter()
            .any(|covered| covered.eq_ignore_ascii_case(owner))
    }
}

/// The tokens a window reads GitHub with. Cheap to clone; the first route is
/// the main sign-in.
#[derive(Clone)]
pub(crate) struct Tokens(Arc<[Route]>);

impl Tokens {
    /// `main` for every owner, except those an additional account covers:
    /// its login, its organizations, and `owners` from its config. The first
    /// listed account covering an owner wins.
    pub(crate) fn new<'a>(
        main: &Profile,
        additional: impl IntoIterator<Item = (&'a Profile, &'a [String])>,
    ) -> Self {
        let main = Route {
            login: main.login.clone(),
            token: main.token.clone(),
            owners: Vec::new(),
        };
        let routes =
            std::iter::once(main).chain(additional.into_iter().map(|(profile, owners)| {
                Route {
                    login: profile.login.clone(),
                    token: profile.token.clone(),
                    owners: std::iter::once(&profile.login)
                        .chain(&profile.organizations)
                        .chain(owners)
                        .cloned()
                        .collect(),
                }
            }));
        Self(routes.collect())
    }

    fn route(&self, owner: &str) -> &Route {
        self.0
            .iter()
            .skip(1)
            .find(|route| route.covers(owner))
            .unwrap_or(&self.0[0])
    }

    /// The token that reads repositories owned by `owner`.
    pub(crate) fn for_owner(&self, owner: &str) -> &Arc<SecretString> {
        &self.route(owner).token
    }

    /// The main sign-in's token.
    pub(crate) fn main(&self) -> &Arc<SecretString> {
        &self.0[0].token
    }

    /// The login of the account that reads repositories owned by `owner`.
    pub(crate) fn login_for(&self, owner: &str) -> &str {
        &self.route(owner).login
    }

    /// The same accounts, owners, and token allocations. A renewed token is a
    /// new allocation, which is how caches notice it, never by its text.
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(other.0.iter()).all(|(this, that)| {
                Arc::ptr_eq(&this.token, &that.token)
                    && this.login == that.login
                    && this.owners == that.owners
            })
    }

    /// Every login, main first: names the account set answers belong to.
    pub(crate) fn key(&self) -> String {
        let logins: Vec<&str> = self.0.iter().map(|route| route.login.as_str()).collect();
        logins.join(",")
    }
}

/// One token read as every owner, as before additional accounts.
impl From<Arc<SecretString>> for Tokens {
    fn from(token: Arc<SecretString>) -> Self {
        Self(Arc::new([Route {
            login: String::new(),
            token,
            owners: Vec::new(),
        }]))
    }
}

#[cfg(test)]
mod tests;
