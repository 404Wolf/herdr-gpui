//! The cache keeps the last read per sign-in, so a restart lists it at
//! once, and the run's own read replaces it.
use super::*;
use crate::cache::{Cache, path_in};
use crate::security::saved::{restore, save};
use std::time::{Instant, SystemTime};

fn names(repos: &[Repo]) -> Vec<&str> {
    repos.iter().map(|repo| repo.name.as_str()).collect()
}

#[test]
fn a_read_is_kept_for_its_sign_in_and_a_failed_repository_keeps_its_last() {
    let dir = tempfile::tempdir().unwrap();
    let cache = Cache::open_at(&path_in(dir.path())).unwrap();
    let me = ["me".to_owned()];
    assert!(restore(&cache, &me).is_none());
    let first = vec![
        repo("a/one", Ok(open(vec![alert(1, Severity::High)]))),
        reported(
            "a/two",
            vec![report("GHSA-aaaa-bbbb-cccc", None, "2026-10-05T00:00:00Z")],
        ),
        repo("a/gone", Ok(Alerts::Off)),
    ];
    save(&cache, &me, &first);
    let (repos, _) = restore(&cache, &me).unwrap();
    assert_eq!(names(&repos), ["a/one", "a/two", "a/gone"]);
    assert_eq!(repos[0].open(), first[0].open());
    assert_eq!(repos[1].reported(), first[1].reported());
    // Nothing that reads GitHub or starts an agent is kept.
    assert!(
        repos
            .iter()
            .all(|repo| repo.account.is_none() && repo.checkouts.is_empty())
    );

    // a/one fails this time and a/gone is no longer read.
    let second = vec![
        repo("a/one", Err(Error::Output)),
        repo("a/two", Ok(Alerts::Off)),
    ];
    save(&cache, &me, &second);
    let (repos, _) = restore(&cache, &me).unwrap();
    assert_eq!(names(&repos), ["a/one", "a/two"]);
    assert_eq!(repos[0].open(), first[0].open());
    assert!(repos[1].reported().is_empty());

    // Another sign-in reads none of it.
    assert!(restore(&cache, &["someone".to_owned()]).is_none());
}

#[test]
fn the_runs_own_read_wins_over_the_cache() {
    let mut security = Security::default();
    let at = SystemTime::UNIX_EPOCH;
    security.restore(vec![repo("a/cached", Ok(Alerts::Off))], at);
    assert_eq!(names(security.repos()), ["a/cached"]);
    assert_eq!(security.read_at(), Some(at));
    // Restoring changes nothing about when the run reads.
    assert!(security.due(Instant::now()));
    security.read_for_test(vec![repo("a/read", Ok(Alerts::Off))]);
    // A restore that lands late is dropped.
    security.restore(vec![repo("a/cached", Ok(Alerts::Off))], at);
    assert_eq!(names(security.repos()), ["a/read"]);
}
