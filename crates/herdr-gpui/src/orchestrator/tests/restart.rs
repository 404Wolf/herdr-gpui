//! A restart shows what the last run found before reading again: the
//! database is the app's own, agent-launcher's is carried over once, the
//! checks are kept beside the items, and the repository a checkout held is
//! cached until git says otherwise.
use super::service::{repository, start, wait};
use super::*;
use crate::cache::{Cache, Kind, path_in};
use crate::orchestrator::{
    location::{database_path_in, legacy_path_in},
    review::{Check, CheckState, PrStatus, Review},
    service::cache_key,
};
use herdr_client::ConnectTarget;
use std::collections::HashMap;

fn status() -> PrStatus {
    PrStatus {
        review: Some(Review::Approved),
        requested: vec!["octocat".into()],
        checks: vec![Check {
            name: "ci".into(),
            state: CheckState::Passed,
        }],
    }
}

#[test]
fn checks_are_kept_per_repository_and_replaced_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db").join("state.sqlite3");
    let mut store = Store::open(&path).unwrap();
    store
        .replace_pr_statuses("g/app", &HashMap::from([(1, status()), (2, status())]))
        .unwrap();
    store
        .replace_pr_statuses("g/other", &HashMap::from([(1, PrStatus::default())]))
        .unwrap();
    store
        .replace_pr_statuses("g/app", &HashMap::from([(2, status())]))
        .unwrap();
    drop(store);
    let statuses = Store::open(&path).unwrap().pr_statuses().unwrap();
    assert_eq!(
        statuses,
        HashMap::from([
            (("g/app".to_owned(), 2), status()),
            (("g/other".to_owned(), 1), PrStatus::default()),
        ])
    );
}

#[test]
fn agent_launchers_database_is_carried_over_once() {
    let dir = tempfile::tempdir().unwrap();
    let checkout = repository(dir.path());
    let git_dir = std::fs::canonicalize(std::path::Path::new(&checkout).join(".git")).unwrap();
    let repo = Repository::Local(git_dir);
    let data = dir.path().join("data");
    let legacy = legacy_path_in(&data, &repo);
    let mut old = Store::open(&legacy).unwrap();
    old.save_run(&run("r1", Owner::HerdrGpui), None).unwrap();
    old.replace_pr_statuses("g/app", &HashMap::from([(1, status())]))
        .unwrap();
    drop(old);

    let service = start(dir.path(), checkout.clone());
    let snapshot = wait(&service, |snapshot| !snapshot.runs.is_empty());
    assert_eq!(snapshot.runs[0].id, "r1");
    assert_eq!(
        snapshot.pr_status.get(&("g/app".to_owned(), 1)),
        Some(&status())
    );
    drop(service);
    let path = database_path_in(&data, &repo);
    assert!(path.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }

    // The copy is the app's now: a run agent-launcher adds later stays there.
    let mut old = Store::open(&legacy).unwrap();
    old.save_run(&run("r2", Owner::HerdrGpui), None).unwrap();
    drop(old);
    let service = start(dir.path(), checkout);
    let snapshot = wait(&service, |snapshot| snapshot.repo.is_some());
    let ids: Vec<_> = snapshot.runs.iter().map(|run| run.id.as_str()).collect();
    assert_eq!(ids, ["r1"]);
}

#[test]
fn the_repository_a_checkout_held_is_cached_until_git_disagrees() {
    let dir = tempfile::tempdir().unwrap();
    let checkout = repository(dir.path());
    let service = start(dir.path(), checkout.clone());
    // Installed agents are looked up once the repository is cached.
    let found = wait(&service, |snapshot| {
        snapshot.installed.is_some() || snapshot.installed_error.is_some()
    })
    .repo
    .unwrap();
    drop(service);
    let cache = Cache::open_at(&path_in(&dir.path().join("data"))).unwrap();
    let key = cache_key(&ConnectTarget::Local, &checkout);
    let entry = cache
        .get::<crate::orchestrator::repo::RepoInfo>(Kind::Repository, &key)
        .unwrap();
    assert_eq!(entry.map(|entry| entry.value), Some(found));

    // The folder is no repository anymore: what was cached goes with it.
    std::fs::remove_dir_all(std::path::Path::new(&checkout).join(".git")).unwrap();
    let service = start(dir.path(), checkout);
    let snapshot = wait(&service, |snapshot| snapshot.error.is_some());
    assert!(matches!(
        snapshot.error.as_deref(),
        Some(Error::NotARepository)
    ));
    assert!(snapshot.repo.is_none());
    assert!(snapshot.items.is_empty());
    assert_eq!(
        cache
            .get::<crate::orchestrator::repo::RepoInfo>(Kind::Repository, &key)
            .unwrap(),
        None
    );
}
