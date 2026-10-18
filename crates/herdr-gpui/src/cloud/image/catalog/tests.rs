#![allow(clippy::unwrap_used)]
use super::*;
use std::{
    path::PathBuf,
    sync::{Mutex, MutexGuard, PoisonError},
};

/// The catalog file tests use instead of the user's, for every thread.
pub(crate) static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

/// A fresh catalog for one test; tests holding one run one at a time.
pub(crate) struct Isolated {
    dir: tempfile::TempDir,
    _serial: MutexGuard<'static, ()>,
}

impl Isolated {
    pub(crate) fn file(&self) -> PathBuf {
        self.dir.path().join(FILE)
    }
}

impl Drop for Isolated {
    fn drop(&mut self) {
        *PATH.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

pub(crate) fn isolated() -> Isolated {
    static SERIAL: Mutex<()> = Mutex::new(());
    let serial = SERIAL.lock().unwrap_or_else(PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    *PATH.lock().unwrap_or_else(PoisonError::into_inner) = Some(dir.path().join(FILE));
    Isolated {
        dir,
        _serial: serial,
    }
}

fn project(id: &str) -> Project {
    Project {
        id: id.into(),
        label: "app".into(),
    }
}

fn ready(provider: CloudProvider, key: &str) -> Image {
    Image {
        provider,
        key: key.into(),
        finished_at: 1,
        outcome: Outcome::Ready {
            reference: format!("img:{key}"),
        },
    }
}

fn origin() -> Origin {
    Origin {
        endpoint_id: "local".into(),
        path: "/src/app".into(),
    }
}

#[test]
fn one_image_per_project_and_provider_is_kept() {
    let _dir = isolated();
    assert!(load().unwrap().is_empty());
    assert_eq!(
        record(
            &project("p"),
            origin(),
            ready(CloudProvider::Container, "k1")
        )
        .unwrap(),
        None
    );
    let replaced = record(
        &project("p"),
        origin(),
        ready(CloudProvider::Container, "k2"),
    )
    .unwrap();
    assert_eq!(replaced, Some(ready(CloudProvider::Container, "k1")));
    record(&project("p"), origin(), ready(CloudProvider::Daytona, "k3")).unwrap();
    record(
        &project("q"),
        origin(),
        ready(CloudProvider::Container, "k4"),
    )
    .unwrap();
    let projects = load().unwrap();
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].images.len(), 2);
    assert_eq!(projects[0].images[0].key, "k2");
}

#[test]
fn forgetting_the_last_image_forgets_the_project() {
    let _dir = isolated();
    record(
        &project("p"),
        origin(),
        ready(CloudProvider::Container, "k"),
    )
    .unwrap();
    assert_eq!(forget("p", CloudProvider::Daytona).unwrap(), None);
    assert_eq!(
        forget("p", CloudProvider::Container).unwrap(),
        Some(ready(CloudProvider::Container, "k"))
    );
    assert!(load().unwrap().is_empty());
    assert_eq!(forget("missing", CloudProvider::Container).unwrap(), None);
}

#[test]
fn a_failed_build_keeps_its_log_and_survives_a_reload() {
    let _dir = isolated();
    let failed = Image {
        outcome: Outcome::Failed {
            log: vec!["step 1".into(), "boom".into()],
        },
        ..ready(CloudProvider::Container, "k")
    };
    record(&project("p"), origin(), failed.clone()).unwrap();
    assert_eq!(load().unwrap()[0].images, [failed]);
}

#[test]
fn a_corrupt_catalog_is_an_error_not_an_empty_one() {
    let catalog = isolated();
    fs::write(catalog.file(), "{").unwrap();
    assert!(matches!(load(), Err(Error::Json(_))));
    fs::write(catalog.file(), r#"{"version": 9, "projects": []}"#).unwrap();
    assert!(matches!(load(), Err(Error::Field(_))));
}
