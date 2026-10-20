use super::*;
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
    time::SystemTime,
};

const MAX_PROJECTS: usize = 2048;
fn io(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// Resolve only an exact reported session ID, never cwd/recency. A second
/// matching file is ambiguity, even if one was modified more recently.
pub fn resolve(config_dir: &Path, id: &str) -> Result<PathBuf> {
    if !valid_session_id(id) {
        return Err(Error::Identity);
    }
    let root = config_dir.join("projects");
    let root = fs::canonicalize(&root).map_err(|e| io(&root, e))?;
    let directories = fs::read_dir(&root).map_err(|e| io(&root, e))?;
    let mut found = None;
    for (i, entry) in directories.enumerate() {
        if i >= MAX_PROJECTS {
            return Err(Error::Limit);
        }
        let entry = entry.map_err(|e| io(&root, e))?;
        let kind = entry.file_type().map_err(|e| io(&entry.path(), e))?;
        if !kind.is_dir() {
            continue;
        }
        let candidate = entry.path().join(format!("{id}.jsonl"));
        let path = match fs::canonicalize(&candidate) {
            Ok(path) => path,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(&candidate, e)),
        };
        if !path.starts_with(&root) {
            return Err(Error::OutsideStore);
        }
        if found.replace(path).is_some() {
            return Err(Error::Ambiguous);
        }
    }
    found.ok_or(Error::Missing)
}

#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
}
fn stamp(metadata: &fs::Metadata) -> Stamp {
    Stamp {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        created: metadata.created().ok(),
    }
}

#[derive(Default)]
pub struct Reader {
    cached: Option<(PathBuf, String, usize, Stamp, Arc<Conversation>)>,
}

impl Reader {
    /// Reads the newest bounded window. `pages` grows only on an explicit Load
    /// older request. Stat-identical files share the immutable parsed result.
    pub fn read(&mut self, path: &Path, id: &str, pages: usize) -> Result<Arc<Conversation>> {
        if !valid_session_id(id) {
            return Err(Error::Identity);
        }
        let budget = pages.clamp(1, MAX_BYTES / PAGE_BYTES) * PAGE_BYTES;
        if !fs::symlink_metadata(path)
            .map_err(|e| io(path, e))?
            .file_type()
            .is_file()
        {
            return Err(Error::OutsideStore);
        }
        let mut file = File::open(path).map_err(|e| io(path, e))?;
        let before = file.metadata().map_err(|e| io(path, e))?;
        if !before.is_file() {
            return Err(Error::OutsideStore);
        }
        let before = stamp(&before);
        if let Some((old_path, old_id, old_budget, old_stamp, conversation)) = &self.cached
            && old_path == path
            && old_id == id
            && *old_budget == budget
            && *old_stamp == before
            && before.modified.is_some()
        {
            return Ok(conversation.clone());
        }
        let start = before.len.saturating_sub(budget as u64);
        file.seek(SeekFrom::Start(start)).map_err(|e| io(path, e))?;
        let mut bytes = Vec::with_capacity((before.len - start) as usize);
        (&mut file)
            .take(before.len - start)
            .read_to_end(&mut bytes)
            .map_err(|e| io(path, e))?;
        let after = stamp(&file.metadata().map_err(|e| io(path, e))?);
        if before != after {
            return Err(Error::Changed);
        }
        let first = if start == 0 {
            0
        } else {
            bytes
                .iter()
                .position(|b| *b == b'\n')
                .map(|i| i + 1)
                .ok_or(Error::Limit)?
        };
        let mut conversation = parse(&bytes[first..], id)?;
        conversation.limited |= start > 0;
        let conversation = Arc::new(conversation);
        self.cached = Some((
            path.to_owned(),
            id.to_owned(),
            budget,
            before,
            conversation.clone(),
        ));
        Ok(conversation)
    }
}
