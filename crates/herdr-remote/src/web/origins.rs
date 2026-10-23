//! Every origin this computer has served the app from. An installed app
//! keeps the origin it was installed from and calls whichever address
//! answers, so when a network change retires that origin it must still pass
//! CORS. Admitting a retired origin is safe: a page there is trusted only
//! with a certificate from this computer's authority, which only this
//! computer holds.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

const FILE_NAME: &str = "origins.json";
/// The most origins kept; the oldest are forgotten first.
pub(crate) const MAX_ORIGINS: usize = 64;

#[derive(Debug)]
pub(crate) struct Origins {
    path: PathBuf,
    /// Newest first.
    seen: Vec<String>,
}

impl Origins {
    /// The origins remembered in `state_dir`; an unreadable file is none.
    /// Blocking.
    pub(crate) fn load(state_dir: &Path) -> Self {
        let path = state_dir.join(FILE_NAME);
        let mut seen: Vec<String> = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        seen.truncate(MAX_ORIGINS);
        Self { path, seen }
    }

    pub(crate) fn contains(&self, origin: &str) -> bool {
        self.seen.iter().any(|seen| seen == origin)
    }

    /// Remembers `current`, newest first, and saves when anything changed.
    /// Blocking.
    pub(crate) fn remember(&mut self, current: &[String]) -> io::Result<()> {
        let mut seen: Vec<String> = current.to_vec();
        seen.extend(
            self.seen
                .iter()
                .filter(|origin| !current.contains(origin))
                .cloned(),
        );
        seen.truncate(MAX_ORIGINS);
        if seen == self.seen {
            return Ok(());
        }
        self.seen = seen;
        let temp = self.path.with_extension("json.tmp");
        fs::write(&temp, serde_json::to_vec(&self.seen)?)?;
        fs::rename(&temp, &self.path)
    }
}
