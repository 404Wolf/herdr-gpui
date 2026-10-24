//! The hosts the loaded config lists, shared by every worker. The app sets
//! them on each config load; workers read a snapshot per request, so a reload
//! never changes a host halfway through one.

use super::config::{Config, HostConfig};
use std::sync::{Arc, RwLock};

/// One configured instance, resolved once at load.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    /// The configured host name, lowercased.
    pub host: String,
    /// Every name remotes may use for it, the host itself first, lowercased.
    pub names: Vec<String>,
    /// The web base, without a trailing slash.
    pub base: String,
    /// Whether `base` is HTTPS, the only scheme a token is sent over.
    pub secure: bool,
    pub token_command: Vec<String>,
    /// A PEM file of certificate authorities trusted beyond the system's.
    pub ca_file: Option<std::path::PathBuf>,
}

impl Host {
    pub fn from_config(config: &HostConfig) -> Option<Self> {
        let base = config.base()?;
        let host = config.host.to_ascii_lowercase();
        let mut names = vec![host.clone()];
        names.extend(
            config
                .aliases
                .iter()
                .map(|alias| alias.to_ascii_lowercase()),
        );
        if let Some(served) = base.host_str().map(str::to_ascii_lowercase)
            && !names.contains(&served)
        {
            names.push(served);
        }
        Some(Self {
            host,
            names,
            secure: base.scheme() == "https",
            base: base.as_str().trim_end_matches('/').to_owned(),
            token_command: config.token_command.clone(),
            ca_file: config.ca_file.as_ref().map(std::path::PathBuf::from),
        })
    }

    /// Whether a remote's host (any case, without a port) names this instance.
    pub fn names_host(&self, host: &str) -> bool {
        self.names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(host))
    }

    /// The API address of `path`, which starts with `/`.
    pub fn api(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    /// The web address of a repository.
    pub fn web(&self, owner: &str, repo: &str) -> String {
        format!("{}/{owner}/{repo}", self.base)
    }
}

static HOSTS: RwLock<Option<Arc<[Host]>>> = RwLock::new(None);

/// Replaces the configured hosts with `config`'s. Hosts a checked config
/// cannot resolve are left out rather than half-configured.
pub fn configure(config: &Config) {
    let hosts: Arc<[Host]> = config.hosts.iter().filter_map(Host::from_config).collect();
    if let Ok(mut current) = HOSTS.write() {
        *current = Some(hosts);
    }
}

/// The configured hosts, empty until a config was loaded.
pub fn hosts() -> Arc<[Host]> {
    HOSTS
        .read()
        .ok()
        .and_then(|hosts| hosts.clone())
        .unwrap_or_else(|| Arc::new([]))
}

/// Whether any host is configured, so GitHub-only setups skip every Gitea
/// step, remote reads included.
pub fn configured() -> bool {
    !hosts().is_empty()
}
