//! The images this app built, filed by project and provider, so Settings can
//! show them and Rebuild knows where the project's recipe is. It is a record,
//! not the truth: before reusing an image the provider is asked whether it
//! still holds it. One image per project and provider is kept.

use super::{Error, Result, key::Project};
use crate::cloud::CloudProvider;
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
};

const FILE: &str = "sandbox-images.json";
/// Room for every project's failed build log at its longest.
const LIMIT: u64 = 16 * 1024 * 1024;
const MAX_PROJECTS: usize = 500;
/// Log lines a failed build keeps for **Log**: its end, which says why.
pub(crate) const LOG_LINES: usize = 40;

/// Where a project's recipe was last read: the endpoint, and the checkout's
/// path on it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Origin {
    pub(crate) endpoint_id: String,
    pub(crate) path: String,
}

/// How a project's latest build for one provider went.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum Outcome {
    /// Built: what new machines start from.
    Ready { reference: String },
    /// The build failed, with the end of its log.
    Failed { log: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Image {
    pub(crate) provider: CloudProvider,
    pub(crate) key: String,
    /// Seconds since the Unix epoch.
    pub(crate) finished_at: u64,
    #[serde(flatten)]
    pub(crate) outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Entry {
    #[serde(flatten)]
    pub(crate) project: Project,
    pub(crate) origin: Origin,
    #[serde(default)]
    pub(crate) images: Vec<Image>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Document {
    version: u32,
    projects: Vec<Entry>,
}

#[cfg(not(test))]
fn path() -> Result<std::path::PathBuf> {
    crate::preferences::state_dir()
        .map(|dir| dir.join(FILE))
        .ok_or(Error::StateDirectory)
}

fn io(path: &Path) -> impl FnOnce(io::Error) -> Error + '_ {
    move |source| Error::Catalog {
        path: path.to_owned(),
        source,
    }
}

fn read(path: &Path) -> Result<Vec<Entry>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io(path)(error)),
    };
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(io(path))?;
    if bytes.len() as u64 > LIMIT {
        return Err(Error::Field(FILE));
    }
    let document: Document = serde_json::from_slice(&bytes).map_err(Error::Json)?;
    if document.version != 1 || document.projects.len() > MAX_PROJECTS {
        return Err(Error::Field(FILE));
    }
    Ok(document.projects)
}

fn write(path: &Path, projects: Vec<Entry>) -> Result<()> {
    let parent = path.parent().ok_or(Error::StateDirectory)?;
    fs::create_dir_all(parent).map_err(io(parent))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io(parent))?;
    serde_json::to_writer_pretty(
        &mut file,
        &Document {
            version: 1,
            projects,
        },
    )
    .map_err(Error::Json)?;
    file.write_all(b"\n").map_err(io(path))?;
    file.as_file().sync_all().map_err(io(path))?;
    file.persist(path).map_err(|error| io(path)(error.error))?;
    Ok(())
}

// Builds and windows each run their own workers; a read-modify-write must not
// interleave.
fn transaction<T>(work: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    static ACCESS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ACCESS.lock().unwrap_or_else(|error| error.into_inner());
    // Tests never touch the user's catalog.
    #[cfg(test)]
    return work(
        &tests::PATH
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or(Error::StateDirectory)?,
    );
    #[cfg(not(test))]
    work(&path()?)
}

/// Every project with its images.
pub(crate) fn load() -> Result<Vec<Entry>> {
    transaction(read)
}

/// Record `image` as `project`'s latest for its provider, its recipe read
/// from `origin`; returns what it replaced, for the caller to delete.
pub(crate) fn record(project: &Project, origin: Origin, image: Image) -> Result<Option<Image>> {
    transaction(|path| {
        let mut projects = read(path)?;
        let index = match projects
            .iter()
            .position(|entry| entry.project.id == project.id)
        {
            Some(index) => index,
            None => {
                if projects.len() >= MAX_PROJECTS {
                    return Err(Error::Field("the number of projects with images"));
                }
                projects.push(Entry {
                    project: project.clone(),
                    origin: origin.clone(),
                    images: Vec::new(),
                });
                projects.len() - 1
            }
        };
        let entry = &mut projects[index];
        entry.project.label.clone_from(&project.label);
        entry.origin = origin;
        let replaced = match entry
            .images
            .iter()
            .position(|old| old.provider == image.provider)
        {
            Some(old) => Some(std::mem::replace(&mut entry.images[old], image)),
            None => {
                entry.images.push(image);
                None
            }
        };
        write(path, projects)?;
        Ok(replaced)
    })
}

/// Forget `project`'s image from `provider`; returns it, for the caller to
/// delete. A project left with no image is forgotten too.
pub(crate) fn forget(project_id: &str, provider: CloudProvider) -> Result<Option<Image>> {
    transaction(|path| {
        let mut projects = read(path)?;
        let Some(index) = projects
            .iter()
            .position(|entry| entry.project.id == project_id)
        else {
            return Ok(None);
        };
        let entry = &mut projects[index];
        let removed = entry
            .images
            .iter()
            .position(|image| image.provider == provider)
            .map(|at| entry.images.remove(at));
        if entry.images.is_empty() {
            projects.remove(index);
        }
        write(path, projects)?;
        Ok(removed)
    })
}

/// Seconds since the Unix epoch, as images record.
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
pub(crate) mod tests;
