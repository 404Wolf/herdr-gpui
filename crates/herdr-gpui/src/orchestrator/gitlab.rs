//! A GitLab project's open issues, read through GitLab's REST API in
//! agent-launcher's GitLab row format: `native_id` is the issue's project
//! `iid`. Every sync is a complete listing, so it may replace the cache.
//!
//! The token comes from `GITLAB_TOKEN` (or `GITLAB_ACCESS_TOKEN`) for the host
//! `GITLAB_HOST` names, gitlab.com by default, else from the GitLab CLI's own
//! sign-in for the remote's host. Without one, public projects still list.

use super::{Activity, Checkpoint, Error, Item, ItemKey, Result, SourceKey};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use std::{
    ffi::OsString,
    io::Read,
    process::Command,
    time::{Duration, Instant, SystemTime},
};
use zeroize::Zeroizing;

const PAGE: usize = 100;
/// The most open issues one sync reads. A project with more is shown but
/// not cached, since a capped listing is no full sync.
pub(super) const MAX_OPEN: usize = 2_000;
const TIMEOUT: Duration = Duration::from_secs(30);
/// One page of 100 issues, descriptions included.
const LIMIT: u64 = 4 * 1024 * 1024;
const CLI_TIMEOUT: Duration = Duration::from_secs(10);
/// A token is at most a few hundred bytes; the rest is room for warnings.
const CLI_OUTPUT_LIMIT: usize = 16 * 1024;
/// Where the GitLab CLI's environment tokens apply when `GITLAB_HOST` is unset.
const DEFAULT_HOST: &str = "gitlab.com";
/// The variables the GitLab CLI reads a token from, in its order.
const TOKEN_VARIABLES: [&str; 2] = ["GITLAB_TOKEN", "GITLAB_ACCESS_TOKEN"];

/// One sync's result. Only a `complete` listing may replace the cache.
pub(crate) struct Listing {
    pub(crate) items: Vec<Item>,
    pub(crate) checkpoint: Checkpoint,
    pub(crate) complete: bool,
}

/// One page of issues, and the page after it when GitLab names one.
pub(super) struct Page {
    pub(super) issues: Vec<IssueNode>,
    pub(super) next: Option<u32>,
}

/// Lists `source`'s open issues, reading GitLab as `token` when given.
/// `previous` keeps agent-launcher's markers, which herdr-gpui never adds.
pub(crate) fn sync(
    source: &SourceKey,
    token: Option<&SecretString>,
    previous: Option<&Checkpoint>,
    cancelled: &impl Fn() -> bool,
    cooldown: &mut Option<Duration>,
) -> Result<Listing> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut fetch = |page: u32| {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        fetch_page(&agent, source, token, page, cooldown)
    };
    full(source, previous.cloned().unwrap_or_default(), &mut fetch)
}

/// Reads every page `fetch` returns, up to [`MAX_OPEN`] issues.
pub(super) fn full(
    source: &SourceKey,
    previous: Checkpoint,
    fetch: &mut impl FnMut(u32) -> Result<Page>,
) -> Result<Listing> {
    let started = Utc::now();
    let mut items = Vec::new();
    let mut complete = true;
    let mut page = 1;
    loop {
        let read = fetch(page)?;
        items.extend(read.issues.into_iter().map(|node| node.into_item(source)));
        match read.next {
            Some(_) if items.len() >= MAX_OPEN => {
                complete = false;
                break;
            }
            Some(next) if next > page => page = next,
            _ => break,
        }
    }
    Ok(Listing {
        items,
        checkpoint: Checkpoint {
            updated_at: Some(started),
            etag: None,
            last_full_at: Some(Utc::now()),
            pr_details: previous.pr_details,
            pr_cursor: previous.pr_cursor,
        },
        complete,
    })
}

/// The API address of `source`'s open issues, page `page`.
pub(super) fn issues_url(source: &SourceKey, page: u32) -> Result<url::Url> {
    let invalid = || Error::RemoteUrl(source.repository.clone());
    let mut url = url::Url::parse(&format!("https://{}/api/v4/projects", source.host))
        .map_err(|_| invalid())?;
    // One segment: the project's full path, its slashes escaped as GitLab asks.
    url.path_segments_mut()
        .map_err(|()| invalid())?
        .push(&source.repository)
        .push("issues");
    url.query_pairs_mut()
        .append_pair("state", "opened")
        .append_pair("scope", "all")
        .append_pair("order_by", "updated_at")
        .append_pair("sort", "desc")
        .append_pair("per_page", &PAGE.to_string())
        .append_pair("page", &page.to_string());
    Ok(url)
}

fn fetch_page(
    agent: &ureq::Agent,
    source: &SourceKey,
    token: Option<&SecretString>,
    page: u32,
    cooldown: &mut Option<Duration>,
) -> Result<Page> {
    let mut request = agent
        .get(issues_url(source, page)?.as_str())
        .header("User-Agent", "Herdr-GPUI")
        .header("Accept", "application/json");
    if let Some(token) = token {
        request = request.header("Authorization", authorization(token)?);
    }
    let mut reply = request
        .call()
        .map_err(|error| Error::GitlabNetwork(Box::new(error)))?;
    let status = reply.status().as_u16();
    let headers = reply.headers();
    if status != 200 {
        *cooldown = retry_after(status, headers, SystemTime::now());
        tracing::warn!(
            category = "orchestrator_gitlab",
            status,
            host = source.host.as_str(),
            "GitLab refused an issue listing"
        );
        return Err(status_error(status, &source.repository, token.is_some()));
    }
    let next = next_page(headers);
    let mut bytes = Vec::new();
    reply
        .body_mut()
        .as_reader()
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(Error::GitlabRead)?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::GitlabSize);
    }
    let issues = serde_json::from_slice(&bytes).map_err(|source| Error::OutputJson {
        operation: "Reading GitLab",
        source,
    })?;
    Ok(Page { issues, next })
}

fn authorization(token: &SecretString) -> Result<ureq::http::HeaderValue> {
    let text = Zeroizing::new(format!("Bearer {}", token.expose_secret()));
    let mut header = ureq::http::HeaderValue::from_str(&text).map_err(|_| Error::GitlabToken)?;
    header.set_sensitive(true);
    Ok(header)
}

/// What a refusal means. GitLab answers a private project as missing to a
/// reader without a token, so that asks for one rather than blaming the remote.
pub(super) fn status_error(status: u16, repository: &str, token: bool) -> Error {
    let repository = repository.to_owned();
    match (status, token) {
        (401 | 403 | 404, false) => Error::GitlabSignIn { repository },
        (401, true) => Error::GitlabAuthentication,
        (403, true) => Error::GitlabForbidden { repository },
        (404, true) => Error::GitlabNotFound { repository },
        (429, _) => Error::GitlabRateLimit,
        (status, _) => Error::GitlabStatus(status),
    }
}

/// GitLab's `x-next-page`, empty on the last page.
pub(super) fn next_page(headers: &ureq::http::HeaderMap) -> Option<u32> {
    headers
        .get("x-next-page")?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// How long a rate-limited project waits: `retry-after` seconds, else until
/// `ratelimit-reset`, bounded to between five minutes and a day.
pub(super) fn retry_after(
    status: u16,
    headers: &ureq::http::HeaderMap,
    now: SystemTime,
) -> Option<Duration> {
    if status != 429 {
        return None;
    }
    let seconds = |name| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    let epoch = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let wait = seconds("retry-after")
        .or_else(|| seconds("ratelimit-reset").map(|reset| reset.saturating_sub(epoch)))
        .unwrap_or(3600);
    Some(Duration::from_secs(wait.clamp(300, 86400)))
}

/// The token to read `host` as: the environment's, when it is meant for this
/// host, else the GitLab CLI's. Blocks on a subprocess: call it from a worker.
pub(crate) fn token(host: &str) -> Option<SecretString> {
    environment_token(host, |name| std::env::var_os(name)).or_else(|| cli_token(host))
}

/// The GitLab CLI's environment token, sent only to the host it names, so a
/// gitlab.com token never reaches a self-managed instance's remote.
pub(super) fn environment_token(
    host: &str,
    variable: impl Fn(&str) -> Option<OsString>,
) -> Option<SecretString> {
    let meant_for = variable("GITLAB_HOST")
        .and_then(|value| value.into_string().ok())
        .map(|value| configured_host(&value))
        .unwrap_or_else(|| DEFAULT_HOST.to_owned());
    if !meant_for.eq_ignore_ascii_case(host) {
        return None;
    }
    TOKEN_VARIABLES
        .iter()
        .filter_map(|name| variable(name)?.into_string().ok())
        .find_map(|value| valid(value.trim()))
}

/// `GITLAB_HOST` may be a bare host or a URL, as the GitLab CLI accepts.
fn configured_host(value: &str) -> String {
    let value = value.trim();
    url::Url::parse(value)
        .ok()
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .and_then(|url| {
            let host = url.host_str()?.to_owned();
            Some(
                url.port()
                    .map_or(host.clone(), |port| format!("{host}:{port}")),
            )
        })
        .unwrap_or_else(|| value.trim_end_matches('/').to_owned())
}

/// `glab config get token --host <host>`, under the bounded process policy
/// shared with PR lookups. Its environment tokens are removed, so the
/// answer is the CLI's saved sign-in for this host alone.
fn cli_token(host: &str) -> Option<SecretString> {
    let mut command = Command::new("glab");
    // An app opened from a desktop launcher has a minimal PATH.
    crate::login_env::apply(&mut command);
    for name in TOKEN_VARIABLES
        .into_iter()
        .chain(["GITLAB_HOST", "OAUTH_TOKEN"])
    {
        command.env_remove(name);
    }
    command.args(["config", "get", "token", "--host", host]);
    let result = crate::pull_request::run_bytes(
        &mut command,
        Instant::now() + CLI_TIMEOUT,
        &|| false,
        CLI_OUTPUT_LIMIT,
    );
    match result {
        Ok((true, output)) => parse_cli(&output),
        Ok((false, _)) => None,
        Err(error) => {
            tracing::debug!(
                category = "orchestrator_gitlab",
                error = %error,
                "The GitLab CLI could not be asked for a token"
            );
            None
        }
    }
}

/// The CLI prints the token alone, or nothing when it holds none.
pub(super) fn parse_cli(output: &[u8]) -> Option<SecretString> {
    let mut lines = std::str::from_utf8(output)
        .ok()?
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    match (lines.next(), lines.next()) {
        (Some(token), None) => valid(token),
        _ => None,
    }
}

/// A token is a short run of visible ASCII; anything else would corrupt the
/// header or is not a token at all.
fn valid(token: &str) -> Option<SecretString> {
    (!token.is_empty() && token.len() <= 1024 && token.bytes().all(|byte| byte.is_ascii_graphic()))
        .then(|| token.into())
}

#[derive(Deserialize)]
struct Author {
    username: String,
}

#[derive(Deserialize)]
pub(super) struct IssueNode {
    iid: u64,
    title: String,
    description: Option<String>,
    state: String,
    web_url: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    author: Option<Author>,
    #[serde(default)]
    labels: Vec<String>,
    user_notes_count: Option<u64>,
}

impl IssueNode {
    pub(super) fn into_item(self, source: &SourceKey) -> Item {
        Item {
            key: ItemKey {
                source: source.clone(),
                native_id: self.iid.to_string(),
            },
            identifier: format!("#{}", self.iid),
            title: self.title,
            description: self.description.filter(|body| !body.is_empty()),
            // GitLab says `opened`; the list reads `open` as GitHub's.
            state: match self.state.as_str() {
                "opened" => "open".to_owned(),
                state => state.to_ascii_lowercase(),
            },
            url: Some(self.web_url),
            author: self.author.map(|author| author.username),
            labels: self.labels,
            parent_id: None,
            blocked_by: Vec::new(),
            priority: None,
            created_at: Some(self.created_at),
            updated_at: Some(self.updated_at),
            pull_request: None,
            activity: Some(Activity {
                comments: self.user_notes_count,
                ..Activity::default()
            }),
        }
    }
}
