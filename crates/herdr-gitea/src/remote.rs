//! Which configured host, owner, and repository a Git remote URL names.
//!
//! Remotes come in three shapes: scp-like `[user@]host:owner/repo`,
//! `ssh://[user@]host[:port]/owner/repo`, and `http(s)://[user[:pw]@]host[:port]/
//! [prefix/]owner/repo`. Only the host decides the instance; userinfo is
//! skipped and never kept, and a port only says where Git connects.

use super::registry::{Host, hosts};

/// A bound on each path segment, above Gitea's own name limits.
const MAX_SEGMENT: usize = 100;

/// A repository on a configured instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub host: Host,
    pub owner: String,
    pub name: String,
}

impl Repo {
    /// `owner/name`, as Gitea's API paths and the orchestrator key it.
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// The API address of a path under this repository, `rest` starting with `/`
    /// or empty. Owner and name passed `segment`, so they need no escaping.
    pub fn api(&self, rest: &str) -> String {
        self.host
            .api(&format!("/repos/{}/{}{rest}", self.owner, self.name))
    }

    pub fn web(&self) -> String {
        self.host.web(&self.owner, &self.name)
    }

    /// Whether `other` is the same repository, as Gitea compares names.
    pub fn same(&self, other: &Self) -> bool {
        self.host.host == other.host.host
            && self.owner.eq_ignore_ascii_case(&other.owner)
            && self.name.eq_ignore_ascii_case(&other.name)
    }
}

/// The repository `remote` names on a configured host, if it is on one.
pub fn repo(remote: &str) -> Option<Repo> {
    repo_in(&hosts(), remote)
}

/// [`repo`] over an explicit host list, for tests and callers holding a snapshot.
pub fn repo_in(hosts: &[Host], remote: &str) -> Option<Repo> {
    let (host, path) = split(remote.trim())?;
    let instance = hosts.iter().find(|candidate| candidate.names_host(host))?;
    // An instance served under a path (`https://example.com/gitea`) prefixes
    // its HTTP remotes with that path; SSH remotes never carry it.
    let prefix = url::Url::parse(&instance.base)
        .ok()
        .map(|base| base.path().trim_matches('/').to_owned())
        .unwrap_or_default();
    let path = path.trim_matches('/');
    let path = if !prefix.is_empty() {
        path.strip_prefix(&format!("{prefix}/")).unwrap_or(path)
    } else {
        path
    };
    let (owner, repo) = path.split_once('/')?;
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if !segment(owner) || !segment(repo) {
        return None;
    }
    Some(Repo {
        host: instance.clone(),
        owner: owner.to_owned(),
        name: repo.to_owned(),
    })
}

/// The configured host a remote names, without resolving a repository.
pub fn host_of(remote: &str) -> Option<Host> {
    let (host, _) = split(remote.trim())?;
    hosts()
        .iter()
        .find(|candidate| candidate.names_host(host))
        .cloned()
}

/// The repository and number of a pull request's web address on a
/// configured host: `<base>/<owner>/<repo>/pulls/<number>`.
pub fn pull_url(url: &str) -> Option<(Repo, u64)> {
    let hosts = hosts();
    let host = hosts
        .iter()
        .find(|host| url.starts_with(&format!("{}/", host.base)))?;
    let rest = &url[host.base.len() + 1..];
    let mut parts = rest.split('/');
    let (Some(owner), Some(repo), Some("pulls"), Some(number), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return None;
    };
    if !segment(owner) || !segment(repo) || number.starts_with('0') {
        return None;
    }
    let number = number.parse().ok().filter(|number| *number > 0)?;
    Some((
        Repo {
            host: host.clone(),
            owner: owner.to_owned(),
            name: repo.to_owned(),
        },
        number,
    ))
}

/// A remote's host, without userinfo or port, and its path.
fn split(remote: &str) -> Option<(&str, &str)> {
    if let Some((scheme, rest)) = remote.split_once("://") {
        if !matches!(
            scheme,
            "ssh" | "git+ssh" | "ssh+git" | "https" | "http" | "git"
        ) {
            return None;
        }
        let (authority, path) = rest.split_once('/')?;
        let host_port = match authority.rsplit_once('@') {
            None => authority,
            Some((userinfo, host)) => {
                let plain =
                    |b: u8| b.is_ascii_graphic() && !matches!(b, b'@' | b'?' | b'#' | b'\\');
                if userinfo.is_empty() || !userinfo.bytes().all(plain) {
                    return None;
                }
                host
            }
        };
        let host = match host_port.rsplit_once(':') {
            Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
            Some(_) => return None,
            None => host_port,
        };
        return (!host.is_empty()).then_some((host, path));
    }
    // scp-like syntax: a colon before any slash, and no scheme.
    let (authority, path) = remote.split_once(':')?;
    if authority.contains('/') {
        return None;
    }
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    (!host.is_empty() && !path.starts_with('/')).then_some((host, path))
}

/// One segment of a remote's path: no grammar is reproduced, but a segment
/// that names a directory itself, or carries punctuation that would not stay
/// inside one URL path segment, is refused.
pub(crate) fn segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SEGMENT
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// A branch or other name as one URL path segment, slashes included.
pub fn encode(name: &str) -> String {
    percent_encoding::utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC).to_string()
}
