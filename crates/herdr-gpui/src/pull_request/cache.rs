//! Bounded, per-checkout PR cache with refresh and error backoff. Entries are
//! capped in number so a long session cannot grow it without limit.
//!
//! What was found outlives the connection it was found over: a reconnect,
//! another device, or a renewed token stops the work in flight but keeps the
//! last answers on screen, so a failing or rate-limited refresh never blanks
//! them. Only another account, or signing out, forgets them.

use super::{
    Input, Lookup, Origin, PullRequest,
    batch::{BATCH_LIMIT, Batch},
    lookup::{Finished, Prefetch},
};
use chrono::{DateTime, DurationRound as _, Local, TimeDelta};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) const CACHE_LIMIT: usize = 128;
pub(super) const REFRESH: Duration = Duration::from_secs(90);
const MISSING_REFRESH: Duration = Duration::from_secs(15);
pub(super) const ERROR_BACKOFF: Duration = Duration::from_secs(300);
/// How long a repository the credential cannot see waits before one branch
/// asks again. Becoming visible takes a user action (installing the GitHub
/// App, a new token), and opening the Git menu asks sooner.
pub(super) const HIDDEN_BACKOFF: Duration = Duration::from_secs(1800);

/// A repository GitHub would not show this credential, paused as a whole so
/// its branches do not each ask on their own backoff.
pub(super) struct HiddenRepository {
    pub(super) repo_key: String,
    pub(super) until: Instant,
    pub(super) message: String,
}

/// Whose lookups the cache serves: the device the checkouts are on and the
/// GitHub account reading them. An entry of another view is kept, so going
/// back shows it at once, but is never shown or refreshed under this one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct View {
    pub(super) origin: Origin,
    pub(super) login: String,
}

/// How many checkouts one request asks for.
pub(super) struct BatchSize(pub(super) usize);

impl Default for BatchSize {
    fn default() -> Self {
        Self(BATCH_LIMIT)
    }
}

pub(super) struct Entry {
    pub(super) view: View,
    pub(super) input: Input,
    pub(super) value: Option<PullRequest>,
    pub(super) message: Option<String>,
    pub(super) checked: Instant,
    pub(super) due: Instant,
    pub(super) used: Instant,
}

/// The scope includes selection epoch, connection generation and daemon boot.
/// Token allocation identity is an additional auth generation, never token text.
#[derive(Default)]
pub(crate) struct Cache {
    pub(super) prefetch: Prefetch,
    pub(super) entries: Vec<Entry>,
    pub(super) queue: std::collections::VecDeque<Input>,
    /// The checkouts of the batch in flight.
    pub(super) active: Vec<Input>,
    pub(super) scope: Option<(u64, u64, String)>,
    pub(super) token: Option<Arc<secrecy::SecretString>>,
    pub(super) view: View,
    pub(super) batch: BatchSize,
    pub(super) next_scan: Option<Instant>,
    pub(super) paused_until: Option<Instant>,
    /// The failure that paused the account, shown with when it resumes.
    pub(super) pause_reason: Option<String>,
    /// Repositories paused after GitHub would not show them, by `repo_key`.
    pub(super) hidden: Vec<HiddenRepository>,
    pub cursor: usize,
}

impl Cache {
    #[cfg(any(test, all(feature = "integration-test", target_os = "macos")))]
    pub fn seed(&mut self, input: Input, value: PullRequest, now: Instant) {
        self.entries
            .retain(|entry| entry.view != self.view || entry.input != input);
        if self.entries.len() == CACHE_LIMIT {
            self.entries.remove(0);
        }
        self.entries.push(Entry {
            view: self.view.clone(),
            input,
            value: Some(value),
            message: None,
            checked: now,
            due: now + REFRESH,
            used: now,
        });
    }

    /// Forgets everything, for signing out or another account.
    pub fn clear(&mut self) {
        self.suspend();
        self.entries.clear();
        self.scope = None;
        self.token = None;
        self.view = View::default();
        self.paused_until = None;
        self.pause_reason = None;
        self.hidden.clear();
        self.cursor = 0;
    }

    /// Stops the batch in flight and the queued lookups but keeps what was
    /// found and any pause, for a connection that dropped or changed. A late
    /// answer to the stopped batch is discarded by its generation.
    pub fn suspend(&mut self) {
        self.prefetch.clear();
        self.queue.clear();
        self.active.clear();
        self.next_scan = None;
    }

    /// Another account starts over. A different device, connection, daemon
    /// boot, or renewed token of the same account stops the work in flight
    /// and keeps what was found, each entry under the device it is on.
    pub fn scope(
        &mut self,
        scope: (u64, u64, String),
        token: Arc<secrecy::SecretString>,
        login: &str,
        origin: Origin,
    ) {
        if self.view.login != login {
            self.clear();
        }
        let view = View {
            origin,
            login: login.to_owned(),
        };
        if self.scope.as_ref() != Some(&scope)
            || self.view != view
            || self
                .token
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, &token))
        {
            self.suspend();
            // Hidden repositories are keyed by a device's paths.
            if self.view.origin != view.origin {
                self.hidden.clear();
            }
            self.scope = Some(scope);
            self.token = Some(token);
            self.view = view;
        }
    }

    /// Drops this device's checkouts that are gone. Other devices' entries
    /// wait to be seen again, bounded by [`CACHE_LIMIT`] with the rest.
    pub fn retain(&mut self, current: impl Fn(&Input) -> bool) {
        let view = &self.view;
        self.entries
            .retain(|entry| &entry.view != view || current(&entry.input));
        self.queue.retain(&current);
        // The rest of the batch is still answered; a gone checkout's answer
        // is ignored when it arrives.
        self.active.retain(&current);
    }

    fn entry(&self, input: &Input) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| entry.view == self.view && &entry.input == input)
    }

    fn entry_mut(&mut self, input: &Input) -> Option<&mut Entry> {
        let view = &self.view;
        self.entries
            .iter_mut()
            .find(|entry| &entry.view == view && &entry.input == input)
    }

    pub fn scan_due(&self, now: Instant) -> bool {
        self.next_scan.is_none_or(|next| now >= next)
    }

    /// A PR may have been created since the last successful empty lookup. Check
    /// the workspace being viewed sooner, without bypassing errors or changing
    /// queue fairness. Only the background poll calls this, never rendering.
    pub fn expire_missing(&mut self, input: &Input, now: Instant) {
        if let Some(entry) = self.entry_mut(input)
            && entry.value.is_none()
            && entry.message.is_none()
            && now >= entry.checked + MISSING_REFRESH
        {
            entry.due = entry.due.min(now);
        }
    }

    /// Request fresh details without discarding the visible value or interrupting
    /// an in-flight lookup. The poll task still owns dispatch and account backoff.
    pub fn refresh(&mut self, input: Input, now: Instant) {
        if self.active.contains(&input) {
            return;
        }
        if let Some(entry) = self.entry_mut(&input) {
            entry.due = now;
        }
        // Asking explicitly is how a newly installed App is noticed.
        if let Some(key) = &input.repo_key {
            self.hidden.retain(|hidden| &hidden.repo_key != key);
        }
        self.queue.retain(|queued| queued != &input);
        self.queue.truncate(CACHE_LIMIT - 1);
        self.queue.push_front(input);
        // Let the next poll dispatch this priority request before a routine scan
        // replaces the queue with the snapshot's round-robin order.
        self.next_scan = Some(now + Duration::from_secs(1));
    }

    pub fn schedule(&mut self, inputs: impl IntoIterator<Item = Input>, now: Instant) {
        self.next_scan = Some(now + Duration::from_secs(1));
        // Replace queued metadata, not an unbounded history of snapshot changes.
        self.queue.clear();
        for input in inputs {
            if self.queue.len() == CACHE_LIMIT {
                break;
            }
            if !self.active.contains(&input)
                && !self.queue.contains(&input)
                && self.entry(&input).is_none_or(|entry| now >= entry.due)
            {
                self.queue.push_back(input);
            }
        }
    }

    pub fn poll(&mut self, now: Instant) -> bool {
        let mut changed = self.collect(now);
        if self.active.is_empty()
            && self.paused_until.is_none_or(|until| now >= until)
            && let Some(token) = self.token.clone()
        {
            let mut batch = Vec::new();
            while batch.len() < self.batch.0.max(1)
                && let Some(input) = self.queue.pop_front()
            {
                // A hidden repository's branches wait with it, without asking.
                if let Some((until, message)) = self.hidden_for(&input, now) {
                    self.record(input, None, Some(message), now, until);
                    changed = true;
                    continue;
                }
                let existing = self.entry(&input);
                if existing.is_some_and(|entry| now < entry.due)
                    || (existing.is_none()
                        && self.entries.len() == CACHE_LIMIT
                        && self.entries.iter().all(|entry| now < entry.due))
                    || batch.contains(&input)
                {
                    continue;
                }
                batch.push(input);
            }
            if !batch.is_empty() {
                self.cursor = self.cursor.wrapping_add(batch.len());
                self.prefetch
                    .request(batch.clone(), self.view.origin.clone(), token);
                self.active = batch;
                // Send it now; only a worker that cannot start answers here.
                self.collect(now);
                changed = true;
            }
        }
        changed
    }

    /// Records the batch in flight once it is answered.
    fn collect(&mut self, now: Instant) -> bool {
        let Some(finished) = self.prefetch.poll() else {
            return false;
        };
        let active = std::mem::take(&mut self.active);
        let (found, failed) = match finished {
            Finished::Answered(batch, cooldown) => {
                // Rate/auth failures pause the account; a bad local repo must
                // not prevent the remaining workspaces from being prefetched.
                if let Some(cooldown) = cooldown {
                    self.paused_until = Some(now + cooldown);
                    self.pause_reason = batch
                        .failed
                        .as_ref()
                        .map(|(_, error)| error.to_string())
                        .or_else(|| {
                            batch.found.iter().find_map(|(_, result)| {
                                result.as_ref().err().map(ToString::to_string)
                            })
                        });
                }
                let Batch { found, failed } = batch;
                (
                    found,
                    failed.map(|(inputs, error)| (inputs, error.to_string())),
                )
            }
            Finished::Failed(message) => (Vec::new(), Some((active.clone(), message.to_owned()))),
        };
        for (input, result) in found {
            if !active.contains(&input) {
                continue;
            }
            match result {
                Ok(value) => self.finish(input, Some(value), None, None, now),
                Err(error) => {
                    let hidden = match &error {
                        crate::Error::GitHubRepositoryHidden { repository, .. } => {
                            Some(repository.clone())
                        }
                        _ => None,
                    };
                    self.finish(input, None, Some(error.to_string()), hidden, now);
                }
            }
        }
        if let Some((inputs, message)) = failed {
            for input in inputs.into_iter().filter(|input| active.contains(input)) {
                self.finish(input, None, Some(message.clone()), None, now);
            }
        }
        true
    }

    /// Stores one checkout's answer and when to ask again: a failure keeps
    /// the visible value, and a hidden repository pauses all its branches.
    fn finish(
        &mut self,
        input: Input,
        value: Option<Option<PullRequest>>,
        message: Option<String>,
        hidden: Option<String>,
        now: Instant,
    ) {
        let due = now
            + match (message.is_some(), &hidden) {
                (_, Some(_)) => HIDDEN_BACKOFF,
                (true, None) => ERROR_BACKOFF,
                (false, None) => REFRESH,
            };
        if let (Some(repository), Some(repo_key), Some(message)) =
            (hidden, input.repo_key.clone(), message.clone())
        {
            self.hide(repository, repo_key, message, due);
        }
        self.record(input, value, message, now, due);
    }

    /// Stores a finished (or skipped) lookup. `value` is `None` to keep the
    /// visible one, as a failure does. A new entry in a full cache evicts the
    /// least recently used one that is due, else the least recently used, so
    /// the cache never outgrows [`CACHE_LIMIT`].
    fn record(
        &mut self,
        input: Input,
        value: Option<Option<PullRequest>>,
        message: Option<String>,
        now: Instant,
        due: Instant,
    ) {
        if let Some(entry) = self.entry_mut(&input) {
            if let Some(value) = value {
                entry.value = value;
            }
            entry.message = message;
            entry.checked = now;
            entry.due = due;
            return;
        }
        if self.entries.len() >= CACHE_LIMIT {
            let least_used = |due_only: bool| {
                self.entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| !due_only || now >= e.due)
                    .min_by_key(|(_, e)| e.used)
                    .map(|(index, _)| index)
            };
            if let Some(index) = least_used(true).or_else(|| least_used(false)) {
                self.entries.remove(index);
            }
        }
        self.entries.push(Entry {
            view: self.view.clone(),
            input,
            value: value.flatten(),
            message,
            checked: now,
            due,
            used: now,
        });
    }

    /// Pauses every branch of `repo_key` until `until`, and shows why on those
    /// already looked up, so none asks GitHub again on its own backoff.
    fn hide(&mut self, repository: String, repo_key: String, message: String, until: Instant) {
        tracing::info!(
            category = "github_pause",
            repository = repository.as_str(),
            seconds = until.saturating_duration_since(Instant::now()).as_secs(),
            "Pausing pull request lookups for a repository GitHub would not show"
        );
        for entry in &mut self.entries {
            if entry.view == self.view && entry.input.repo_key.as_deref() == Some(repo_key.as_str())
            {
                entry.message = Some(message.clone());
                entry.due = entry.due.max(until);
            }
        }
        self.hidden.retain(|hidden| hidden.repo_key != repo_key);
        if self.hidden.len() == CACHE_LIMIT {
            self.hidden.remove(0);
        }
        self.hidden.push(HiddenRepository {
            repo_key,
            until,
            message,
        });
    }

    /// When `input`'s repository resumes and why it is paused, while it is.
    fn hidden_for(&mut self, input: &Input, now: Instant) -> Option<(Instant, String)> {
        self.hidden.retain(|hidden| now < hidden.until);
        let key = input.repo_key.as_deref()?;
        self.hidden
            .iter()
            .find(|hidden| hidden.repo_key == key)
            .map(|hidden| (hidden.until, hidden.message.clone()))
    }

    /// Whether a lookup for `input` is in flight or queued to dispatch, so the
    /// chrome can say it is waiting on GitHub. A paused account is not loading.
    pub fn loading(&self, input: &Input, now: Instant) -> bool {
        self.active.contains(input)
            || (self.token.is_some()
                && self.paused_until.is_none_or(|until| now >= until)
                && self.queue.contains(input))
    }

    /// Pure cache read for chrome painted every frame: never schedules work and
    /// never reorders the cache, so rendering cannot start a request.
    pub fn peek(&self, repo_key: &str, branch: &str) -> Option<&PullRequest> {
        self.entries
            .iter()
            .find(|entry| {
                entry.view == self.view
                    && entry.input.checkout.is_none()
                    && entry.input.repo_key.as_deref() == Some(repo_key)
                    && entry.input.branch == branch
            })
            .and_then(|entry| entry.value.as_ref())
    }

    /// Pure cache read of one checkout's lookup, local checkouts included:
    /// `None` until it was looked up once, else its pull request (if any)
    /// and why its latest refresh failed (if it did).
    #[cfg(unix)]
    pub fn lookup(&self, input: &Input) -> Option<(Option<&PullRequest>, Option<&str>)> {
        self.entry(input)
            .map(|entry| (entry.value.as_ref(), entry.message.as_deref()))
    }

    /// Pure cache read by lookup key, for the merged worktree cleanup.
    pub fn get(&self, input: &Input) -> Option<&PullRequest> {
        self.entry(input).and_then(|entry| entry.value.as_ref())
    }

    /// Pure cache read: opening a menu cannot launch Git, HTTPS or daemon requests.
    pub fn present(&mut self, input: &Input, view: &mut Lookup, now: Instant) {
        view.clear();
        if let Some(entry) = self.entry_mut(input) {
            entry.used = now;
            view.value = entry.value.clone();
            view.message = entry.message.clone();
            view.checked = Some(entry.checked);
        } else if let Some(until) = self.paused_until.filter(|until| now < *until) {
            view.message = Some(paused(
                until - now,
                self.pause_reason.as_deref(),
                Local::now(),
            ));
        } else {
            view.loading = true;
        }
        // A stale value stays visible, but the reason it is stale must too.
        if view.value.is_some()
            && let Some(message) = view.message.take()
        {
            view.message = Some(format!(
                "Refresh unavailable, showing the last result. {message}"
            ));
        }
    }
}

/// Why GitHub requests are paused and the local minute they resume, rounded
/// up so the shown time is never early.
pub(super) fn paused(wait: Duration, reason: Option<&str>, now: DateTime<Local>) -> String {
    let resume = TimeDelta::from_std(wait)
        .ok()
        .and_then(|wait| now.checked_add_signed(wait))
        .map(|at| at.duration_round_up(TimeDelta::minutes(1)).unwrap_or(at));
    let when = resume.map_or("later".to_owned(), |at| {
        format!("at {}", at.format("%H:%M"))
    });
    match reason {
        Some(reason) => format!("{reason} GitHub requests paused; retrying {when}."),
        None => format!(
            "GitHub requests paused after an authentication or rate-limit error; retrying {when}."
        ),
    }
}
