//! `[gitea]` in config-gpui: the Gitea or Forgejo hosts whose remotes the GUI
//! reads pull requests, checks, reviews, and issues from.
//!
//! ```toml
//! [[gitea.hosts]]
//! host = "gitea.example.com"
//! # url = "https://gitea.example.com"  # web and API base; default https://<host>
//! # aliases = ["gitea-ssh"]            # other names remotes use for this host
//! # ca_file = "/etc/ssl/my-ca.pem"     # a CA beyond the system's to trust
//! token_command = ["cat", "/run/secrets/gitea-token"]
//! ```
//!
//! A host is given a command that prints its token, never the token itself:
//! the file is meant to be shared and kept in dotfiles, so a credential pasted
//! into it would be refused like one under `[github]`.

use serde::Deserialize;

/// The most hosts one config lists; more is a mistake, not a setup.
pub const MAX_HOSTS: usize = 16;
/// The most words in a token command.
const MAX_COMMAND: usize = 32;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub hosts: Vec<HostConfig>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HostConfig {
    /// The host remotes name, without a scheme or port, e.g. `gitea.example.com`.
    pub host: String,
    /// The web address Gitea is served at, which the API lives under
    /// (`<url>/api/v1`). `https://<host>` when unset.
    pub url: Option<String>,
    /// Other host names remotes reach this instance by, such as an SSH alias.
    pub aliases: Vec<String>,
    /// A program and its arguments that print the API token on stdout. Unset
    /// reads anonymously, which only public repositories allow.
    pub token_command: Vec<String>,
    /// A PEM file of certificate authorities to trust for this host, beyond
    /// the system's CA bundle, for an instance signed by a private CA.
    pub ca_file: Option<String>,
}

/// Why `[gitea]` was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("[gitea] lists more than {MAX_HOSTS} hosts")]
    TooMany,
    #[error("[[gitea.hosts]] host {0:?} is not a host name")]
    Host(String),
    #[error("[[gitea.hosts]] host {0:?} is listed twice")]
    Duplicate(String),
    #[error("[[gitea.hosts]] url {0:?} must be an http:// or https:// address")]
    Url(String),
    #[error("[[gitea.hosts]] token_command for {0:?} is too long")]
    Command(String),
    #[error("[[gitea.hosts]] ca_file {0:?} must be an absolute path")]
    CaFile(String),
}

impl Config {
    /// Checks every host, so a typo is reported at load rather than as a
    /// remote that silently stays unrecognized.
    pub fn check(&self) -> Result<(), ConfigError> {
        if self.hosts.len() > MAX_HOSTS {
            return Err(ConfigError::TooMany);
        }
        let mut seen: Vec<String> = Vec::new();
        for host in &self.hosts {
            for name in std::iter::once(&host.host).chain(&host.aliases) {
                if !host_name(name) {
                    return Err(ConfigError::Host(name.clone()));
                }
                let name = name.to_ascii_lowercase();
                if seen.contains(&name) {
                    return Err(ConfigError::Duplicate(name));
                }
                seen.push(name);
            }
            if let Some(url) = &host.url
                && host.base().is_none()
            {
                return Err(ConfigError::Url(url.clone()));
            }
            if let Some(path) = &host.ca_file
                && !std::path::Path::new(path).is_absolute()
            {
                return Err(ConfigError::CaFile(path.clone()));
            }
            if host.token_command.len() > MAX_COMMAND
                || host.token_command.first().is_some_and(String::is_empty)
            {
                return Err(ConfigError::Command(host.host.clone()));
            }
        }
        Ok(())
    }
}

impl HostConfig {
    /// The web base without a trailing slash: `url`, else `https://<host>`.
    pub fn base(&self) -> Option<url::Url> {
        let text = self
            .url
            .clone()
            .unwrap_or_else(|| format!("https://{}", self.host));
        let mut url = url::Url::parse(text.trim()).ok()?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let path = url.path().trim_end_matches('/').to_owned();
        url.set_path(&path);
        Some(url)
    }
}

/// A DNS name or address as a remote names its host: letters, digits, dots,
/// and dashes, without a scheme, port, or path.
pub(crate) fn host_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && !name.starts_with(['.', '-'])
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
}
