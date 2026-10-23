//! A host that cannot be reached answered nothing about the checkout, so
//! the repository last shown stays, beside the failure, until a sync finds
//! it again; a host that answers otherwise still drops it.
use super::*;
use crate::cache::path_in;
use std::process::{Command as Process, ExitStatus};

fn exited(code: u8) -> ExitStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        ExitStatus::from_raw(i32::from(code) << 8)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::ExitStatusExt;
        ExitStatus::from_raw(u32::from(code))
    }
}

/// Detection whose `ssh` could not reach the host.
fn unreached() -> Error {
    Error::Script {
        operation: "Reading the repository",
        source: herdr_client::Error::ScriptExit {
            status: exited(255),
            stderr: "kex_exchange_identification: read: Connection reset by peer".into(),
        },
    }
}

/// A worker for a fresh repository in `dir`, with its repository found,
/// shown, and cached.
fn opened(dir: &std::path::Path) -> (Service, Worker, Host) {
    let checkout = dir.join("app");
    std::fs::create_dir_all(&checkout).unwrap();
    for args in [
        &["init", "-q"][..],
        &[
            "remote",
            "add",
            "origin",
            "git@github.acme.com:group/app.git",
        ],
    ] {
        let status = Process::new("git")
            .arg("-C")
            .arg(&checkout)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }
    let (service, mut worker, _) = Service::prepare(Request {
        target: ConnectTarget::Local,
        checkout: checkout.to_string_lossy().into_owned(),
        workspace_id: "w1".into(),
        token: None,
        data_root: Some(dir.join("data")),
        timing: Timing {
            reload: Duration::from_millis(20),
            sync: Duration::from_secs(3600),
        },
    });
    worker.open().unwrap();
    assert!(worker.snapshot.repo.is_some());
    let host = Host::new(&ConnectTarget::Local).unwrap();
    (service, worker, host)
}

fn cached(worker: &Worker) -> Option<RepoInfo> {
    let cache = Cache::open_at(&path_in(worker.request.data_root.as_ref().unwrap())).unwrap();
    let key = cache_key(&worker.request.target, &worker.request.checkout);
    cache
        .get::<RepoInfo>(Kind::Repository, &key)
        .unwrap()
        .map(|entry| entry.value)
}

#[test]
fn an_unreached_host_keeps_the_repository_shown_until_a_sync_finds_it() {
    let dir = tempfile::tempdir().unwrap();
    let (_service, mut worker, host) = opened(dir.path());
    let shown = worker.snapshot.repo.clone();
    worker.found(&host, Err(unreached())).unwrap();
    assert_eq!(worker.snapshot.repo, shown, "the listing stays");
    assert!(worker.store.is_some());
    assert!(
        worker
            .snapshot
            .error
            .as_deref()
            .is_some_and(|error| matches!(
                error,
                Error::Script { source, .. } if source.host_unreached()
            ))
    );
    assert_eq!(cached(&worker), shown, "the next start shows it too");
    // The next sync finds the repository first, and the failure goes.
    worker.sync(None);
    assert!(!worker.unreached);
    assert!(worker.snapshot.error.is_none());
    assert_eq!(worker.snapshot.repo, shown);
}

#[test]
fn an_unreached_host_with_nothing_shown_is_found_on_a_later_sync() {
    let dir = tempfile::tempdir().unwrap();
    let (_service, mut worker, host) = opened(dir.path());
    let found = worker.snapshot.repo.clone();
    worker.forget_repository();
    worker.found(&host, Err(unreached())).unwrap();
    assert!(worker.snapshot.repo.is_none());
    assert!(worker.snapshot.error.is_some());
    worker.sync(None);
    assert!(worker.snapshot.error.is_none());
    assert_eq!(worker.snapshot.repo, found);
}

#[test]
fn a_host_that_answers_otherwise_still_drops_the_repository() {
    let dir = tempfile::tempdir().unwrap();
    let (_service, mut worker, host) = opened(dir.path());
    worker.found(&host, Err(unreached())).unwrap();
    let error = worker.found(&host, Err(Error::NotARepository)).unwrap_err();
    assert!(matches!(error, Error::NotARepository));
    assert!(!worker.unreached, "a later sync does not look again");
    assert!(worker.snapshot.repo.is_none());
    assert!(worker.store.is_none());
    assert_eq!(cached(&worker), None);
}
