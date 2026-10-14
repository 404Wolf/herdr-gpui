//! Bounded, per-checkout PR cache with refresh and error backoff. Entries are
//! capped in number so a long session cannot grow it without limit.

use super::{Input, Lookup, Origin, PullRequest};
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

pub(super) struct Entry {
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
    pub(super) lookup: Lookup,
    pub(super) entries: Vec<Entry>,
    pub(super) queue: std::collections::VecDeque<Input>,
    pub(super) active: Option<Input>,
    pub(super) scope: Option<(u64, u64, String)>,
    pub(super) token: Option<Arc<secrecy::SecretString>>,
    pub(super) origin: Origin,
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
        self.entries.retain(|entry| entry.input != input);
        if self.entries.len() == CACHE_LIMIT {
            self.entries.remove(0);
        }
        self.entries.push(Entry {
            input,
            value: Some(value),
            message: None,
            checked: now,
            due: now + REFRESH,
            used: now,
        });
    }

    pub fn clear(&mut self) {
        self.lookup.clear();
        self.entries.clear();
        self.queue.clear();
        self.active = None;
        self.scope = None;
        self.token = None;
        self.origin = Origin::Local;
        self.next_scan = None;
        self.paused_until = None;
        self.pause_reason = None;
        self.hidden.clear();
        self.cursor = 0;
    }

    /// A different device, daemon boot, account, or origin starts over.
    pub fn scope(
        &mut self,
        scope: (u64, u64, String),
        token: Arc<secrecy::SecretString>,
        origin: Origin,
    ) {
        if self.scope.as_ref() != Some(&scope)
            || self.origin != origin
            || self
                .token
                .as_ref()
                .is_none_or(|old| !Arc::ptr_eq(old, &token))
        {
            self.clear();
            self.scope = Some(scope);
            self.token = Some(token);
            self.origin = origin;
        }
    }

    pub fn retain(&mut self, current: impl Fn(&Input) -> bool) {
        self.entries.retain(|entry| current(&entry.input));
        self.queue.retain(&current);
        if self.active.as_ref().is_some_and(|input| !current(input)) {
            self.lookup.clear();
            self.active = None;
        }
    }

    pub fn scan_due(&self, now: Instant) -> bool {
        self.next_scan.is_none_or(|next| now >= next)
    }

    /// A PR may have been created since the last successful empty lookup. Check
    /// the workspace being viewed sooner, without bypassing errors or changing
    /// queue fairness. Only the background poll calls this, never rendering.
    pub fn expire_missing(&mut self, input: &Input, now: Instant) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| &entry.input == input)
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
        if self.active.as_ref() == Some(&input) {
            return;
        }
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.input == input) {
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
            if self.active.as_ref() != Some(&input)
                && !self.queue.contains(&input)
                && self
                    .entries
                    .iter()
                    .find(|entry| entry.input == input)
                    .is_none_or(|entry| now >= entry.due)
            {
                self.queue.push_back(input);
            }
        }
    }

    pub fn poll(&mut self, now: Instant) -> bool {
        let mut changed = self.lookup.poll();
        if !self.lookup.loading
            && let Some(input) = self.active.take()
        {
            let failed = self.lookup.message.is_some();
            // Rate/auth failures pause the account; a bad local repo must not
            // prevent the remaining workspaces from being prefetched.
            if let Some(cooldown) = self.lookup.cooldown.take() {
                self.paused_until = Some(now + cooldown);
                self.pause_reason = self.lookup.message.clone();
            }
            let hidden = self.lookup.hidden.take();
            let due = now
                + match (failed, &hidden) {
                    (_, Some(_)) => HIDDEN_BACKOFF,
                    (true, None) => ERROR_BACKOFF,
                    (false, None) => REFRESH,
                };
            let value = self.lookup.value.take();
            let message = self.lookup.message.take();
            if let (Some(repository), Some(repo_key), Some(message)) =
                (hidden, input.repo_key.clone(), message.clone())
            {
                self.hide(repository, repo_key, message, due);
            }
            self.record(input, (!failed).then_some(value), message, now, due);
            changed = true;
        }
        if self.active.is_none()
            && self.paused_until.is_none_or(|until| now >= until)
            && let Some(token) = self.token.clone()
        {
            while let Some(input) = self.queue.pop_front() {
                // A hidden repository's branches wait with it, without asking.
                if let Some((until, message)) = self.hidden_for(&input, now) {
                    self.record(input, None, Some(message), now, until);
                    changed = true;
                    continue;
                }
                let existing = self.entries.iter().find(|entry| entry.input == input);
                if existing.is_some_and(|entry| now < entry.due)
                    || (existing.is_none()
                        && self.entries.len() == CACHE_LIMIT
                        && self.entries.iter().all(|entry| now < entry.due))
                {
                    continue;
                }
                self.lookup.clear();
                self.lookup
                    .request(input.clone(), self.origin.clone(), token.clone());
                self.active = Some(input);
                self.cursor = self.cursor.wrapping_add(1);
                self.lookup.poll();
                changed = true;
                break;
            }
        }
        changed
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
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.input == input) {
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
            if entry.input.repo_key.as_deref() == Some(repo_key.as_str()) {
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
        self.active.as_ref() == Some(input)
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
                entry.input.checkout.is_none()
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
        self.entries
            .iter()
            .find(|entry| &entry.input == input)
            .map(|entry| (entry.value.as_ref(), entry.message.as_deref()))
    }

    /// Pure cache read by lookup key, for the merged worktree cleanup.
    pub fn get(&self, input: &Input) -> Option<&PullRequest> {
        self.entries
            .iter()
            .find(|entry| &entry.input == input)
            .and_then(|entry| entry.value.as_ref())
    }

    /// Pure cache read: opening a menu cannot launch Git, HTTPS or daemon requests.
    pub fn present(&mut self, input: &Input, view: &mut Lookup, now: Instant) {
        view.clear();
        if let Some(entry) = self.entries.iter_mut().find(|entry| &entry.input == input) {
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
