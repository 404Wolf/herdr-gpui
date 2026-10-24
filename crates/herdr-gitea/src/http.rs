//! Bounded HTTP against one Gitea host: size-capped bodies, a deadline per
//! call, a sensitive `Authorization` header, numbered pages, and status codes
//! turned into errors that say what the user can do.

use super::{Error, Result, registry::Host, token::token};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    io::Read,
    sync::Arc,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

/// The page size asked for. Gitea caps pages at its `MAX_RESPONSE_ITEMS`
/// (50 by default), so a shorter page than asked is not the last one; only
/// an empty page or the total count ends a listing.
pub const PAGE: usize = 50;
/// The most pages one listing reads.
pub const MAX_PAGES: usize = 20;
/// One page of 50 pull requests, bodies included, stays far below this.
const LIMIT: u64 = 4 * 1024 * 1024;

/// One host's agent and token, for the requests of one task.
pub struct Client {
    host: Host,
    agent: ureq::Agent,
    token: Option<Arc<SecretString>>,
    deadline: Instant,
}

impl Client {
    /// A client for `host` whose requests end by `timeout` from now. Reads the
    /// host's token first, which may run its command.
    pub fn new(host: &Host, timeout: Duration) -> Result<Self> {
        Ok(Self::with_token(host, token(host)?, timeout))
    }

    pub fn with_token(host: &Host, token: Option<Arc<SecretString>>, timeout: Duration) -> Self {
        Self {
            host: host.clone(),
            agent: ureq::Agent::config_builder()
                .tls_config(
                    ureq::tls::TlsConfig::builder()
                        .root_certs(super::tls::roots(host.ca_file.as_deref()))
                        .build(),
                )
                .timeout_global(Some(timeout))
                .max_redirects(0)
                .http_status_as_error(false)
                .build()
                .into(),
            token,
            deadline: Instant::now() + timeout,
        }
    }

    pub fn host(&self) -> &Host {
        &self.host
    }

    /// Whether requests carry a token, so a refusal can say why.
    pub fn signed_in(&self) -> bool {
        self.token.is_some()
    }

    /// GET `url` as JSON. Read anonymously, Gitea answers a private
    /// repository as missing, so that asks for a token instead.
    pub fn get<T: DeserializeOwned>(&self, url: &str, cancelled: &impl Fn() -> bool) -> Result<T> {
        match self.get_optional(url, cancelled)? {
            Some(value) => Ok(value),
            None => Err(missing(url, self.signed_in())),
        }
    }

    /// GET `url`, `None` when Gitea says it does not exist.
    pub fn get_optional<T: DeserializeOwned>(
        &self,
        url: &str,
        cancelled: &impl Fn() -> bool,
    ) -> Result<Option<T>> {
        match self.get_value(url, cancelled) {
            Ok((value, _)) => serde_json::from_value(value).map(Some).map_err(Error::Json),
            Err(Error::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Every page of a listing at `url` (which may carry a query), up to
    /// `max_pages`. Returns the items and whether the listing was complete.
    pub fn get_pages<T: DeserializeOwned>(
        &self,
        url: &str,
        max_pages: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Result<(Vec<T>, bool)> {
        let separator = if url.contains('?') { '&' } else { '?' };
        let mut items = Vec::new();
        for page in 1..=max_pages.max(1) {
            let page_url = format!("{url}{separator}limit={PAGE}&page={page}");
            let (value, total) =
                self.get_value(&page_url, cancelled)
                    .map_err(|error| match error {
                        Error::NotFound(_) => missing(url, self.signed_in()),
                        error => error,
                    })?;
            let page_items: Vec<T> = serde_json::from_value(value).map_err(Error::Json)?;
            let empty = page_items.is_empty();
            items.extend(page_items);
            if empty || total.is_some_and(|total| items.len() >= total) {
                return Ok((items, true));
            }
        }
        Ok((items, false))
    }

    /// POST `body` as JSON to `url`; the answer's JSON, or `Null` when empty.
    pub fn post(&self, url: &str, body: &Value) -> Result<Value> {
        self.check_deadline()?;
        let text = serde_json::to_string(body).map_err(Error::Json)?;
        let mut request = self
            .agent
            .post(url)
            .header("User-Agent", "Herdr-GPUI")
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");
        if let Some(header) = self.authorization()? {
            request = request.header("Authorization", header);
        }
        let reply = request
            .send(text.as_bytes())
            .map_err(|error| Error::Network(Box::new(error)))?;
        let (value, _) = self.read(url, reply)?;
        Ok(value)
    }

    fn get_value(
        &self,
        url: &str,
        cancelled: &impl Fn() -> bool,
    ) -> Result<(Value, Option<usize>)> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        self.check_deadline()?;
        let mut request = self
            .agent
            .get(url)
            .header("User-Agent", "Herdr-GPUI")
            .header("Accept", "application/json");
        if let Some(header) = self.authorization()? {
            request = request.header("Authorization", header);
        }
        let reply = request
            .call()
            .map_err(|error| Error::Network(Box::new(error)))?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        self.read(url, reply)
    }

    fn check_deadline(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            return Err(Error::Timeout);
        }
        Ok(())
    }

    fn authorization(&self) -> Result<Option<ureq::http::HeaderValue>> {
        let Some(token) = &self.token else {
            return Ok(None);
        };
        let text = Zeroizing::new(format!("token {}", token.expose_secret()));
        let mut header = ureq::http::HeaderValue::from_str(&text)
            .map_err(|_| Error::Token(self.host.host.clone()))?;
        header.set_sensitive(true);
        Ok(Some(header))
    }

    fn read(
        &self,
        url: &str,
        mut reply: ureq::http::Response<ureq::Body>,
    ) -> Result<(Value, Option<usize>)> {
        let status = reply.status().as_u16();
        let total = reply
            .headers()
            .get("x-total-count")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok());
        let mut bytes = Zeroizing::new(Vec::new());
        reply
            .body_mut()
            .as_reader()
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(Error::Read)?;
        if bytes.len() as u64 > LIMIT {
            return Err(Error::Size);
        }
        if !(200..=299).contains(&status) {
            // Only the path is logged: the host's query strings carry no
            // secrets, but bodies may echo what was sent.
            tracing::debug!(
                category = "gitea_http",
                status,
                host = self.host.host.as_str(),
                "Gitea refused a request"
            );
            return Err(status_error(status, url, &bytes));
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok((Value::Null, total));
        }
        let value = serde_json::from_slice(&bytes).map_err(Error::Json)?;
        Ok((value, total))
    }
}

/// What a refusal means.
pub(crate) fn status_error(status: u16, url: &str, body: &[u8]) -> Error {
    let path = path(url);
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| value["message"].as_str().map(clean))
        .unwrap_or_default();
    match status {
        401 => Error::Authentication(path),
        403 => Error::Forbidden(path),
        404 => Error::NotFound(path),
        405 | 409 | 422 if !message.is_empty() => Error::Refused(message),
        429 => Error::RateLimit,
        status => Error::Status(status),
    }
}

/// Something a read needs is missing: to an anonymous reader Gitea answers a
/// private repository that way, so that asks for a token.
fn missing(url: &str, signed_in: bool) -> Error {
    if signed_in {
        Error::NotFound(path(url))
    } else {
        Error::Authentication(path(url))
    }
}

fn path(url: &str) -> String {
    url::Url::parse(url)
        .map(|url| url.path().to_owned())
        .unwrap_or_default()
}

/// Untrusted remote text for one line of an error.
fn clean(text: &str) -> String {
    text.chars()
        .take(200)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
