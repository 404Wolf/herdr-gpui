//! Builds running now, shared by the whole app: a dispatch and the Settings
//! card can ask for the same image, and both then wait on, and show, one
//! build. A build runs on the worker that started it; others block until it
//! ends. Views follow builds through [`watch`], which wakes on any change.

use super::{
    Error, Recipe, Support,
    catalog::{self, Image, Outcome},
    key::Project,
    progress::Progress,
};
use crate::cloud::CloudProvider;
use std::{
    collections::VecDeque,
    sync::{Condvar, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

/// A build running now, as views show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Running {
    pub(crate) provider: CloudProvider,
    pub(crate) project: Project,
    pub(crate) key: String,
    pub(crate) progress: Progress,
    /// The end of its log, for **View log** while it runs.
    pub(crate) log: VecDeque<String>,
}

#[derive(Default)]
struct State {
    /// Bumped on every change, so a view redraws only for one.
    revision: u64,
    /// Bumped when the catalog changes, so a view rereads it only then
    /// rather than for every line a build prints.
    catalog: u64,
    running: Vec<Running>,
}

static STATE: Mutex<State> = Mutex::new(State {
    revision: 0,
    catalog: 0,
    running: Vec::new(),
});
/// Log lines a running build keeps for **Log**.
const LOG_LINES: usize = 200;
/// The longest log line kept, so a build printing long lines cannot swell
/// the catalog.
const LINE_LIMIT: usize = 300;
static CHANGED: Condvar = Condvar::new();

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn changed(state: &mut State) {
    state.revision += 1;
    CHANGED.notify_all();
}

/// The builds running now, and the revision they are at.
pub(crate) fn snapshot() -> (u64, Vec<Running>) {
    let state = state();
    (state.revision, state.running.clone())
}

/// The catalog's revision: it changes when an image is recorded or reset.
pub(crate) fn catalog_revision() -> u64 {
    state().catalog
}

/// Wait until something changes after `revision`, or `timeout` passes.
/// Blocks: run it on a worker.
pub(crate) fn watch(revision: u64, timeout: Duration) -> (u64, Vec<Running>) {
    let state = state();
    let (state, _) = CHANGED
        .wait_timeout_while(state, timeout, |state| state.revision == revision)
        .unwrap_or_else(PoisonError::into_inner);
    (state.revision, state.running.clone())
}

/// Removes the build from the running list however it ends.
struct Registered {
    provider: CloudProvider,
    project_id: String,
}

impl Drop for Registered {
    fn drop(&mut self) {
        let mut state = state();
        state.running.retain(|running| {
            !(running.provider == self.provider && running.project.id == self.project_id)
        });
        changed(&mut state);
    }
}

/// Take a line of the build's output; returns its progress after it.
fn update(provider: CloudProvider, project_id: &str, line: &str) -> Option<Progress> {
    let mut state = state();
    let running = state
        .running
        .iter_mut()
        .find(|running| running.provider == provider && running.project.id == project_id)?;
    for line in line.lines() {
        running.progress.line(line);
        if running.log.len() == LOG_LINES {
            running.log.pop_front();
        }
        running.log.push_back(
            line.chars()
                .filter(|c| !c.is_control())
                .take(LINE_LIMIT)
                .collect(),
        );
    }
    let progress = running.progress.clone();
    changed(&mut state);
    Some(progress)
}

/// The image `recipe` makes on `provider`, built unless the provider already
/// holds one with the same key; returns what new machines start from. A
/// build of the same project already running is waited for first. Blocks:
/// run it on a worker.
pub(crate) fn ensure(
    provider: CloudProvider,
    support: &Support,
    recipe: &Recipe,
    cancelled: &dyn Fn() -> bool,
    report: &dyn Fn(&Progress),
) -> crate::Result<String> {
    if !support.context && !recipe.files.is_empty() {
        return Err(Error::Context(crate::cloud::name(provider)).into());
    }
    let platform = (support.platform)(cancelled)?;
    let key = super::key::key(&recipe.dockerfile, &recipe.files, &platform);
    let name = (support.name)(&recipe.project, &key);
    let busy = |state: &State| {
        state
            .running
            .iter()
            .any(|running| running.provider == provider && running.project.id == recipe.project.id)
    };
    let registered = loop {
        {
            let mut state = state();
            while busy(&state) {
                if cancelled() {
                    return Err(Error::Cancelled.into());
                }
                state = CHANGED
                    .wait_timeout(state, Duration::from_millis(500))
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
            }
        }
        // Built already, here or by the build just waited for. Asked with
        // the lock released: views read it, and the provider may be slow.
        if let Some(reference) = reusable(provider, support, recipe, &key, &name, cancelled)? {
            return Ok(reference);
        }
        let mut state = state();
        // Another worker started one meanwhile: wait for it instead.
        if busy(&state) {
            continue;
        }
        state.running.push(Running {
            provider,
            project: recipe.project.clone(),
            key: key.clone(),
            progress: Progress {
                step: None,
                status: "Starting the build…".into(),
            },
            log: VecDeque::new(),
        });
        changed(&mut state);
        break Registered {
            provider,
            project_id: recipe.project.id.clone(),
        };
    };
    let log = |line: &str| {
        if let Some(progress) = update(provider, &recipe.project.id, line) {
            report(&progress);
        }
    };
    let built = (support.build)(&name, recipe, cancelled, &log);
    let tail = |extra: Option<String>| {
        let mut log: Vec<String> = snapshot()
            .1
            .into_iter()
            .find(|running| running.provider == provider && running.project.id == recipe.project.id)
            .map(|running| running.log.into())
            .unwrap_or_default();
        log.extend(extra);
        // The end says why; the catalog keeps only that much.
        let excess = log.len().saturating_sub(catalog::LOG_LINES);
        log.drain(..excess);
        log
    };
    let outcome = match &built {
        Ok(reference) => Outcome::Ready {
            reference: reference.clone(),
        },
        Err(error) => Outcome::Failed {
            log: tail(Some(error.to_string())),
        },
    };
    let replaced = catalog::record(
        &recipe.project,
        recipe.origin.clone(),
        Image {
            provider,
            key,
            finished_at: catalog::now(),
            outcome,
        },
    );
    state().catalog += 1;
    drop(registered);
    let reference = built?;
    if let Some(Image {
        outcome: Outcome::Ready { reference: old },
        ..
    }) = replaced?
        && old != reference
        && let Err(error) = (support.delete)(&old, &|| false)
    {
        tracing::warn!(category = "sandbox_image", %error, "Could not delete a replaced image");
    }
    Ok(reference)
}

/// The image `name` when the catalog records it built with `key` and the
/// provider still holds it.
fn reusable(
    provider: CloudProvider,
    support: &Support,
    recipe: &Recipe,
    key: &str,
    name: &str,
    cancelled: &dyn Fn() -> bool,
) -> crate::Result<Option<String>> {
    let recorded = catalog::load()?
        .into_iter()
        .find(|entry| entry.project.id == recipe.project.id)
        .and_then(|entry| {
            entry
                .images
                .into_iter()
                .find(|image| image.provider == provider && image.key == key)
        });
    if !matches!(
        recorded,
        Some(Image {
            outcome: Outcome::Ready { .. },
            ..
        })
    ) {
        return Ok(None);
    }
    (support.find)(name, cancelled)
}

/// Delete `project_id`'s image on `provider` and forget it, so the next
/// dispatch builds afresh. A build running for it is left to finish.
pub(crate) fn reset(
    provider: CloudProvider,
    support: &Support,
    project_id: &str,
) -> crate::Result<()> {
    let busy = state()
        .running
        .iter()
        .any(|running| running.provider == provider && running.project.id == project_id);
    if busy {
        return Err(Error::Busy.into());
    }
    if let Some(Image {
        outcome: Outcome::Ready { reference },
        ..
    }) = catalog::forget(project_id, provider)?
    {
        (support.delete)(&reference, &|| false)?;
    }
    let mut state = state();
    state.catalog += 1;
    changed(&mut state);
    Ok(())
}

#[cfg(test)]
mod tests;
