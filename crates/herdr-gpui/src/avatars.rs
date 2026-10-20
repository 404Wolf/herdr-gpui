use gpui::{Image, ImageFormat};
mod cache;
use herdr_client::{ConnectTarget, protocol::ClientShellWorkspace};
use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant, SystemTime},
};

const MAX_REQUESTS: usize = 128;

/// A bound on each remote path segment, above GitHub's own name limits.
const MAX_SEGMENT: usize = 100;

/// The whole budget for reading one remote checkout's Git configuration.
const REMOTE_TIMEOUT: Duration = Duration::from_secs(10);

/// A checkout whose repository owner's avatar a sidebar row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Checkout<S> {
    /// A workspace directory on this machine, read by local Git.
    Local(S),
    /// The Git directory a saved SSH device's daemon reported, read there.
    Ssh { target: S, git_dir: S },
}

impl<'a> Checkout<&'a str> {
    /// The checkout behind `workspace` on the device reached by `target`.
    /// Only the local daemon's directories are read by this machine's Git; a
    /// saved SSH device is read over SSH by the Git directory its daemon
    /// reported, so it needs worktree metadata. Other devices may be forwarded
    /// from an unknown machine and get none.
    pub(crate) fn of(
        local: bool,
        target: &'a ConnectTarget,
        workspace: &'a ClientShellWorkspace,
    ) -> Option<Self> {
        match target {
            ConnectTarget::Ssh { target, .. } => {
                workspace.worktree.as_ref().map(|tree| Self::Ssh {
                    target,
                    git_dir: &tree.key,
                })
            }
            _ if local => Some(Self::Local(&workspace.new_workspace_cwd)),
            _ => None,
        }
    }

    fn to_owned(self) -> Checkout<String> {
        match self {
            Self::Local(cwd) => Checkout::Local(cwd.to_owned()),
            Self::Ssh { target, git_dir } => Checkout::Ssh {
                target: target.to_owned(),
                git_dir: git_dir.to_owned(),
            },
        }
    }
}

impl Checkout<String> {
    fn as_deref(&self) -> Checkout<&str> {
        match self {
            Self::Local(cwd) => Checkout::Local(cwd),
            Self::Ssh { target, git_dir } => Checkout::Ssh { target, git_dir },
        }
    }
}

type Slot = Option<Arc<Image>>;

/// Memory front end to the shared public-image disk cache.
pub struct Avatars {
    requests: mpsc::SyncSender<Checkout<String>>,
    results: mpsc::Receiver<(Checkout<String>, Slot)>,
    // Presence deduplicates pending requests as well as resolved hits and misses.
    local: HashMap<String, Slot>,
    // Keyed by SSH target, then Git directory, so a row looks up without allocating.
    remote: HashMap<String, HashMap<String, Slot>>,
}

impl Avatars {
    pub fn new() -> Self {
        let (requests, incoming) = mpsc::sync_channel::<Checkout<String>>(MAX_REQUESTS);
        let (outgoing, results) = mpsc::sync_channel(MAX_REQUESTS * 2);
        let (refresh, network) = mpsc::sync_channel::<(Checkout<String>, String)>(MAX_REQUESTS);
        let updated = outgoing.clone();
        thread::spawn(move || {
            let agent = avatar_agent();
            let mut owners = HashMap::new();
            for (checkout, url) in network {
                let image = owners
                    .entry(url.clone())
                    .or_insert_with(|| download(&agent, &url))
                    .clone();
                if let Some(image) = image
                    && updated.send((checkout, Some(image))).is_err()
                {
                    break;
                }
            }
        });
        // Deliberately detach: dropping the sender ends the worker without a UI join.
        thread::spawn(move || {
            for checkout in incoming {
                let url = repo_owner(checkout.as_deref())
                    .map(|owner| format!("https://avatars.githubusercontent.com/{owner}?size=48"));
                let cached = url.as_deref().and_then(cached_image);
                let fresh = cached.as_ref().is_some_and(|(_, fresh)| *fresh);
                if outgoing
                    .send((checkout.clone(), cached.map(|(image, _)| image)))
                    .is_err()
                {
                    break;
                }
                if !fresh && let Some(url) = url {
                    let _ = refresh.try_send((checkout, url));
                }
            }
        });
        Self {
            requests,
            results,
            local: HashMap::new(),
            remote: HashMap::new(),
        }
    }

    /// Enqueue each exact checkout at most once, including failed resolutions.
    pub(crate) fn request(&mut self, checkout: Checkout<&str>) {
        let len = self.local.len() + self.remote.values().map(HashMap::len).sum::<usize>();
        if len >= MAX_REQUESTS || self.slot(checkout).is_some() {
            return;
        }
        if self.requests.try_send(checkout.to_owned()).is_ok() {
            self.insert(checkout.to_owned(), None);
        }
    }

    /// Drain completed requests without blocking. Misses also count as updates.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok((checkout, image)) = self.results.try_recv() {
            self.insert(checkout, image);
            changed = true;
        }
        changed
    }

    pub(crate) fn image(&self, checkout: Checkout<&str>) -> Option<Arc<Image>> {
        self.slot(checkout).cloned().flatten()
    }

    fn slot(&self, checkout: Checkout<&str>) -> Option<&Slot> {
        match checkout {
            Checkout::Local(cwd) => self.local.get(cwd),
            Checkout::Ssh { target, git_dir } => self.remote.get(target)?.get(git_dir),
        }
    }

    fn insert(&mut self, checkout: Checkout<String>, image: Slot) {
        match checkout {
            Checkout::Local(cwd) => {
                self.local.insert(cwd, image);
            }
            Checkout::Ssh { target, git_dir } => {
                self.remote
                    .entry(target)
                    .or_default()
                    .insert(git_dir, image);
            }
        }
    }
}

/// The GitHub owner behind a checkout's `upstream` remote, else its `origin`.
fn repo_owner(checkout: Checkout<&str>) -> Option<String> {
    let deadline = Instant::now() + REMOTE_TIMEOUT;
    ["remote.upstream.url", "remote.origin.url"]
        .into_iter()
        .find_map(|key| {
            let url = match checkout {
                Checkout::Local(cwd) => local_config(cwd, key),
                // The upstream read may use most of the budget; origin gets
                // only what is left.
                Checkout::Ssh { target, git_dir } => herdr_client::remote_config_value(
                    target,
                    git_dir,
                    key,
                    deadline.checked_duration_since(Instant::now())?,
                    || false,
                )
                .ok()
                .flatten(),
            }?;
            github_owner(&url)
        })
}

fn local_config(cwd: &str, key: &str) -> Option<String> {
    let output = herdr_client::new_command("git")
        .args(["-C", cwd, "config", "--default", "", "--get", key])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let text = std::str::from_utf8(&output.stdout).ok()?;
    Some(text.trim_end_matches(['\r', '\n']).to_owned())
}

fn github_owner(remote: &str) -> Option<String> {
    github_repo(remote).map(|(owner, _)| owner)
}

/// One segment of a remote's path.
///
/// Account and repository names are GitHub's to define, so no name grammar is
/// reproduced here: a segment is refused only when it names the directory
/// itself or carries punctuation that would not stay inside its own path
/// segment in the avatar URL built from it.
fn segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SEGMENT
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub(crate) fn github_repo(remote: &str) -> Option<(String, String)> {
    let path = ["git@github.com:", "ssh://git@github.com/"]
        .into_iter()
        .find_map(|prefix| remote.strip_prefix(prefix))
        .or_else(|| http_path(remote))?;
    let (owner, repo) = path.split_once('/')?;
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if !segment(owner) || !segment(repo) {
        return None;
    }
    Some((owner.to_ascii_lowercase(), repo.to_owned()))
}

/// The path of an `http(s)://[userinfo@]github.com/` remote.
///
/// Git accepts a user name, or a user name and token, before the host
/// (`https://user@github.com/o/r`, `https://user:token@github.com/o/r`).
/// The host is what Git connects to, so it alone must be `github.com`; the
/// userinfo is skipped and never kept. Userinfo that a URL parser could read
/// as the end of the authority (`/`, `?`, `#`, `\`, a second `@`) is refused
/// so the host cannot be misread.
fn http_path(remote: &str) -> Option<&str> {
    let rest = ["https://", "http://"]
        .into_iter()
        .find_map(|scheme| remote.strip_prefix(scheme))?;
    let (authority, path) = rest.split_once('/')?;
    let host = match authority.split_once('@') {
        None => authority,
        Some((userinfo, host)) => {
            let plain = |b: u8| b.is_ascii_graphic() && !matches!(b, b'@' | b'?' | b'#' | b'\\');
            if userinfo.is_empty() || !userinfo.bytes().all(plain) {
                return None;
            }
            host
        }
    };
    (host == "github.com").then_some(path)
}

fn cached_image(url: &str) -> Option<(Arc<Image>, bool)> {
    if !valid_profile_avatar(url) {
        return None;
    }
    cache::read(&cache::root()?, url, SystemTime::now())
}

pub(crate) type AvatarUpdates = mpsc::Receiver<Arc<Image>>;

/// Called by the profile worker after authenticated identity resolution.
pub(crate) fn profile_avatar(url: &str) -> (Option<Arc<Image>>, Option<AvatarUpdates>) {
    profile_avatar_with(cache::root(), url, SystemTime::now(), |url| {
        download(&avatar_agent(), &url)
    })
}

fn profile_avatar_with(
    root: Option<std::path::PathBuf>,
    url: &str,
    now: SystemTime,
    fetch: impl FnOnce(String) -> Option<Arc<Image>> + Send + 'static,
) -> (Option<Arc<Image>>, Option<AvatarUpdates>) {
    if !valid_profile_avatar(url) {
        return (None, None);
    }
    let cached = root.as_deref().and_then(|path| cache::read(path, url, now));
    let fresh = cached.as_ref().is_some_and(|(_, fresh)| *fresh);
    let image = cached.map(|(image, _)| image);
    if fresh {
        return (image, None);
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let url = url.to_owned();
    let _ = thread::Builder::new()
        .name("herdr-profile-avatar".into())
        .spawn(move || {
            if let Some(image) = fetch(url) {
                let _ = tx.send(image);
            }
        });
    (image, Some(rx))
}

fn avatar_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .max_redirects(0)
        .build()
        .into()
}

fn valid_profile_avatar(url: &str) -> bool {
    url.len() <= 2048
        && url
            .strip_prefix("https://avatars.githubusercontent.com/")
            .is_some_and(|path| {
                !path.is_empty()
                    && path
                        .bytes()
                        .all(|b| b.is_ascii_graphic() && !matches!(b, b'\\' | b'#'))
            })
}

fn download(agent: &ureq::Agent, url: &str) -> Option<Arc<Image>> {
    if !valid_profile_avatar(url) {
        return None;
    }
    let mut response = agent.get(url).call().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let content_type = response.headers().get("content-type")?.to_str().ok()?;
    let format = avatar_format(content_type)?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(cache::LIMIT as u64)
        .read_to_vec()
        .ok()?;
    let image = cache::decode(&bytes)?;
    if image.format != format {
        return None;
    }
    if let Some(root) = cache::root() {
        let _ = cache::write(&root, url, &bytes, SystemTime::now());
    }
    Some(image)
}

fn avatar_format(content_type: &str) -> Option<ImageFormat> {
    match content_type
        .split(';')
        .next()?
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
