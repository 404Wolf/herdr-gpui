//! The OpenSandbox REST calls this app makes: create, read, resume, list, and
//! delete a sandbox, and find its execd endpoint.

use super::{
    Error, Result, Settings, Status,
    http::{self, Auth},
    settings::sandbox_id,
};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// The metadata key naming the device a sandbox was made for.
pub(crate) const LABEL: &str = "herdr-gpui-device";
/// execd's port inside every sandbox.
pub(crate) const EXECD_PORT: u16 = 44772;
const START_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Keeps Herdr's server running once the installer has put it in place, and
/// starts it again after a resume: the sandbox's main process.
const ENTRYPOINT: &str = r#"while :; do if [ -x "$HOME/.local/bin/herdr" ]; then "$HOME/.local/bin/herdr" server >/dev/null 2>&1; fi; sleep 2; done"#;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Sandbox {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) status: State,
    #[serde(default)]
    pub(crate) metadata: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct State {
    #[serde(default)]
    pub(crate) state: String,
    #[serde(default)]
    pub(crate) message: Option<String>,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    items: Vec<Sandbox>,
}

/// Where a service in a sandbox is reached, and the headers it needs.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Endpoint {
    pub(crate) endpoint: String,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
}

/// One server, authenticated by its API key.
pub(crate) struct Api<'a> {
    pub(crate) settings: &'a Settings,
    pub(crate) key: &'a SecretString,
}

fn path(id: &str) -> Result<String> {
    if !sandbox_id(id) {
        return Err(Error::Field("sandbox ID"));
    }
    Ok(format!("/sandboxes/{id}"))
}

impl Api<'_> {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.settings.base)
    }

    fn auth(&self) -> Auth<'_> {
        Auth::Key(self.key)
    }

    /// Make a sandbox for the device `name`, without an expiry: it lasts until
    /// deleted.
    pub(crate) fn create(&self, name: &str) -> Result<Sandbox> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Create<'a> {
            image: Image<'a>,
            entrypoint: [&'a str; 3],
            resource_limits: Limits<'a>,
            metadata: BTreeMap<&'a str, &'a str>,
        }
        #[derive(Serialize)]
        struct Image<'a> {
            uri: &'a str,
        }
        #[derive(Serialize)]
        struct Limits<'a> {
            cpu: &'a str,
            memory: &'a str,
        }
        http::post(
            "create sandbox",
            &self.url("/sandboxes"),
            &self.auth(),
            &Create {
                image: Image {
                    uri: &self.settings.image,
                },
                entrypoint: ["/bin/sh", "-c", ENTRYPOINT],
                resource_limits: Limits {
                    cpu: &self.settings.cpu,
                    memory: &self.settings.memory,
                },
                metadata: BTreeMap::from([(LABEL, name)]),
            },
        )
    }

    /// The sandbox `id`, or [`Error::Gone`] when it no longer exists.
    pub(crate) fn sandbox(&self, id: &str) -> Result<Sandbox> {
        match http::get("read sandbox", &self.url(&path(id)?), &self.auth()) {
            Err(Error::Status(Status { code: 404, .. })) => Err(Error::Gone(id.to_owned())),
            other => other,
        }
    }

    /// Wait until the sandbox `id` runs, resuming it if it was paused.
    pub(crate) fn wait_running(&self, id: &str, cancelled: &dyn Fn() -> bool) -> Result<()> {
        let deadline = Instant::now() + START_TIMEOUT;
        let mut resumed = false;
        loop {
            let sandbox = self.sandbox(id)?;
            match sandbox.status.state.as_str() {
                "Running" => return Ok(()),
                "Paused" if !resumed => {
                    http::post_empty(
                        "resume sandbox",
                        &self.url(&format!("{}/resume", path(id)?)),
                        &self.auth(),
                    )?;
                    resumed = true;
                }
                "Failed" | "Terminated" | "Deleted" => {
                    return Err(Error::Stopped(
                        sandbox.status.message.unwrap_or(sandbox.status.state),
                    ));
                }
                _ => {}
            }
            if cancelled() {
                return Err(Error::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(Error::StartTimeout);
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    /// The sandboxes this app made, at most the first page of them.
    pub(crate) fn ours(&self) -> Result<Vec<Sandbox>> {
        let page: Page = http::get(
            "list sandboxes",
            &self.url("/sandboxes?pageSize=200"),
            &self.auth(),
        )?;
        Ok(page
            .items
            .into_iter()
            .filter(|sandbox| sandbox.metadata.contains_key(LABEL))
            .collect())
    }

    pub(crate) fn delete_sandbox(&self, id: &str) -> Result<()> {
        http::delete("delete sandbox", &self.url(&path(id)?), &self.auth())
    }

    /// The sandbox's execd, as a URL root with the headers it needs.
    pub(crate) fn execd(&self, id: &str) -> Result<Execd> {
        let endpoint: Endpoint = http::get(
            "find the sandbox's execd",
            &self.url(&format!("{}/endpoints/{EXECD_PORT}", path(id)?)),
            &self.auth(),
        )?;
        let root = format!(
            "{}://{}",
            self.settings.endpoint_scheme(),
            endpoint.endpoint.trim_end_matches('/')
        );
        let parsed = url::Url::parse(&root).map_err(|_| Error::Field("execd endpoint"))?;
        if parsed.host().is_none() || parsed.query().is_some() || !parsed.username().is_empty() {
            return Err(Error::Field("execd endpoint"));
        }
        Ok(Execd {
            root,
            headers: endpoint.headers,
        })
    }
}

/// A sandbox's execd: its URL root and the headers each request carries.
#[derive(Clone, Debug)]
pub(crate) struct Execd {
    pub(crate) root: String,
    pub(crate) headers: BTreeMap<String, String>,
}
