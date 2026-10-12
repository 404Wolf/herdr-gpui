//! The Boat REST calls this app makes: list, read, create, resume, rename, and
//! authorize the app's SSH key. Readiness maps Boat lifecycle states onto what
//! adding or connecting needs to know.

use super::{Error, Result, http::Request};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const START_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const POLL: Duration = Duration::from_secs(3);
const LIST_LIMIT: usize = 100;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct Sandbox {
    pub(crate) id: String,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) state: String,
    #[serde(default)]
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Readiness {
    Ready,
    Stopped,
    Pending(String),
    Failed(String),
    Deleted,
}

impl Sandbox {
    pub(crate) fn readiness(&self) -> Readiness {
        match self.state.as_str() {
            "ready" | "idle" | "running" => Readiness::Ready,
            "archived" => Readiness::Stopped,
            "error" => Readiness::Failed(
                self.error
                    .clone()
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or_else(|| self.state.clone()),
            ),
            "cancelled" => Readiness::Deleted,
            other => Readiness::Pending(other.replace('_', " ")),
        }
    }
}

#[derive(Deserialize)]
struct List {
    sandboxes: Vec<Sandbox>,
}

#[derive(Deserialize)]
struct One {
    sandbox: Sandbox,
}

#[derive(Serialize)]
struct Create<'a> {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    machine_type: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    environment: Option<&'a str>,
}

#[derive(Serialize)]
struct Rename<'a> {
    name: &'a str,
}

#[derive(Serialize)]
struct PublicKey<'a> {
    key: &'a str,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SshAccess {
    pub(crate) machine_ip: Option<String>,
    pub(crate) ssh_endpoint: Option<String>,
    pub(crate) ssh_user: String,
    pub(crate) host_key: String,
}

fn segment(id: &str) -> Result<&str> {
    const ALPHABET: &[u8] = b"23456789abcdefghjkmnpqrstuvwxyz";
    if id
        .strip_prefix("bx_")
        .is_some_and(|value| value.len() == 8 && value.bytes().all(|byte| ALPHABET.contains(&byte)))
    {
        Ok(id)
    } else {
        Err(Error::Field("Boat sandbox ID"))
    }
}

impl Request<'_> {
    pub(crate) fn sandboxes(&self) -> Result<Vec<Sandbox>> {
        let reply: List = self.get("sandboxes", &format!("/sandboxes?limit={LIST_LIMIT}"))?;
        Ok(reply.sandboxes)
    }

    pub(crate) fn sandbox(&self, id: &str) -> Result<Sandbox> {
        self.get::<One>("sandbox", &format!("/sandboxes/{}", segment(id)?))
            .map(|reply| reply.sandbox)
            .map_err(|error| match error {
                Error::Status(super::Status { code: 404, .. }) => Error::Deleted,
                error => error,
            })
    }

    pub(crate) fn create(&self, name: &str, cancelled: impl Fn() -> bool) -> Result<Sandbox> {
        let idempotency = uuid::Uuid::new_v4().hyphenated().to_string();
        let payload = Create {
            machine_type: self.settings.machine_type.as_deref(),
            environment: self.settings.environment.as_deref(),
        };
        let send = || self.post_idempotent("create_sandbox", "/sandboxes", &idempotency, &payload);
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut created: Result<One> = send();
        // A lost reply may still have created a billable sandbox. Every retry
        // keeps the same key, even if cancelled after sending the first request.
        for delay in [500, 1_000, 2_000] {
            if !matches!(
                &created,
                Err(Error::Network(_) | Error::Read(_))
                    | Err(Error::Status(super::Status {
                        code: 409 | 500..=599,
                        ..
                    }))
            ) {
                break;
            }
            std::thread::sleep(Duration::from_millis(delay));
            created = send();
        }
        let created = created?;
        // Boat assigns a default name during create and exposes renaming as a
        // separate operation. If naming alone fails, keep the usable sandbox.
        match self.patch::<One>(
            "rename_sandbox",
            &format!("/sandboxes/{}", segment(&created.sandbox.id)?),
            &Rename { name },
        ) {
            Ok(reply) => Ok(reply.sandbox),
            Err(error) => {
                tracing::warn!(category = "boat_api", %error, "Boat sandbox could not be renamed");
                Ok(created.sandbox)
            }
        }
    }

    pub(crate) fn resume(&self, id: &str) -> Result<()> {
        self.post::<One>(
            "resume_sandbox",
            &format!("/sandboxes/{}/resume", segment(id)?),
            &serde_json::json!({}),
        )
        .map(drop)
    }

    pub(crate) fn ssh_access(&self, id: &str, public_key: &str) -> Result<SshAccess> {
        self.post(
            "ssh_access",
            &format!("/sandboxes/{}/sshkey", segment(id)?),
            &PublicKey { key: public_key },
        )
    }
}

pub(crate) fn wait_ready(
    request: &Request,
    id: &str,
    cancelled: impl Fn() -> bool,
    mut progress: impl FnMut(&Readiness),
) -> Result<Sandbox> {
    let deadline = Instant::now() + START_TIMEOUT;
    let mut resumed = false;
    loop {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let sandbox = request.sandbox(id)?;
        let readiness = sandbox.readiness();
        match &readiness {
            Readiness::Ready => return Ok(sandbox),
            Readiness::Failed(reason) => return Err(Error::Sandbox(reason.clone())),
            Readiness::Deleted => return Err(Error::Deleted),
            Readiness::Stopped if !resumed => {
                resumed = true;
                request.resume(id)?;
            }
            Readiness::Stopped | Readiness::Pending(_) => {}
        }
        progress(&readiness);
        if Instant::now() >= deadline {
            return Err(Error::StartTimeout);
        }
        let wake = Instant::now() + POLL;
        while Instant::now() < wake {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests;
