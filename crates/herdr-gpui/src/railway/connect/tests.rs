#![allow(clippy::unwrap_used)]
use super::*;
use crate::railway::identity::{Record, Source, keys_dir};
use std::{fs, sync::atomic::AtomicBool};

fn record(source: Source, window_ends_by: Option<u64>) -> Record {
    Record {
        id: "key-abcd".into(),
        source,
        attached: true,
        window_ends_by,
        claimed: false,
    }
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = walk(dir);
    names.sort();
    names
}

fn walk(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .flat_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path)
            } else {
                vec![path.display().to_string()]
            }
        })
        .collect()
}

fn connect(dir: &Path, now: u64) -> Result<PathBuf> {
    key_to_use(dir, "key-abcd", now, |_, _| Ok(true))
}

#[test]
fn a_device_without_an_identity_is_never_given_one() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(connect(dir.path(), 0), Err(Error::NoIdentity)));
    assert!(files(dir.path()).is_empty(), "reconnecting creates nothing");
}

#[test]
fn a_missing_key_is_reported_not_replaced() {
    let dir = tempfile::tempdir().unwrap();
    for source in [
        Source::Generated,
        Source::Borrowed {
            path: dir.path().join("gone").display().to_string(),
        },
    ] {
        identity::save(dir.path(), record(source, Some(u64::MAX))).unwrap();
        let before = files(dir.path());
        assert!(matches!(connect(dir.path(), 0), Err(Error::KeyMissing(_))));
        assert_eq!(files(dir.path()), before, "no key was generated");
        assert!(!keys_dir(dir.path()).exists());
    }
}

#[test]
fn after_the_window_a_reconnect_stops_instead_of_risking_a_new_vm() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    fs::write(&key, "key").unwrap();
    let borrowed = Source::Borrowed {
        path: key.display().to_string(),
    };
    identity::save(dir.path(), record(borrowed.clone(), Some(1000))).unwrap();
    assert!(matches!(connect(dir.path(), 1001), Err(Error::WindowEnded)));
    // Never reached by setup: no basis to assume the VM still exists.
    identity::save(dir.path(), record(borrowed.clone(), None)).unwrap();
    assert!(matches!(connect(dir.path(), 0), Err(Error::WindowEnded)));
    // Within the window, or claimed, the saved key is the one to use.
    identity::save(dir.path(), record(borrowed.clone(), Some(1000))).unwrap();
    assert_eq!(connect(dir.path(), 1000).unwrap(), key);
    let mut claimed = record(borrowed, None);
    claimed.claimed = true;
    identity::save(dir.path(), claimed).unwrap();
    assert_eq!(connect(dir.path(), u64::MAX).unwrap(), key);
}

#[test]
fn another_service_is_refused() {
    let stop = AtomicBool::new(true);
    let Err(error) = super::connect("https://app.daytona.io/api", "key-abcd", "default", &stop)
    else {
        panic!("a different account must not reach railway.new");
    };
    assert!(error.get_ref().is_some_and(|inner| inner.is::<Error>()));
}

#[test]
fn a_replaced_key_file_never_reaches_another_vm() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    fs::write(&key, "key").unwrap();
    let mut saved = record(
        Source::Borrowed {
            path: key.display().to_string(),
        },
        Some(u64::MAX),
    );
    saved.id = "key-changed1".into();
    identity::save(dir.path(), saved).unwrap();
    let seen = std::cell::RefCell::new(None);
    let result = key_to_use(dir.path(), "key-changed1", 0, |path, id| {
        *seen.borrow_mut() = Some((path.to_owned(), id.to_owned()));
        Ok(false)
    });
    assert!(matches!(result, Err(Error::KeyChanged(path)) if path == key));
    assert_eq!(seen.into_inner(), Some((key, "key-changed1".to_owned())));
}

#[test]
fn refusals_retrying_cannot_fix_switch_the_device_off_but_outages_do_not() {
    use herdr_client::{Error as Client, SshFailure};
    let refused = |failure| Error::Bridge(Client::SshRefused(failure));
    for permanent_failure in [
        Error::WindowEnded,
        Error::KeyChanged("/k".into()),
        refused(SshFailure::Auth),
        refused(SshFailure::HostKey),
        refused(SshFailure::HerdrMissing),
        refused(SshFailure::Other),
    ] {
        assert!(permanent(&permanent_failure), "{permanent_failure:?}");
    }
    for transient in [
        refused(SshFailure::Unreachable),
        refused(SshFailure::NoRoute),
        Error::KeyMissing("/k".into()),
        Error::NoIdentity,
        Error::Suspended,
    ] {
        assert!(!permanent(&transient), "{transient:?}");
    }
}

#[test]
fn a_suspended_device_stays_off_until_set_up_again() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("key");
    fs::write(&key, "key").unwrap();
    let mut saved = record(
        Source::Borrowed {
            path: key.display().to_string(),
        },
        Some(u64::MAX),
    );
    saved.id = "key-suspend1".into();
    identity::save(dir.path(), saved).unwrap();
    let connect = || key_to_use(dir.path(), "key-suspend1", 0, |_, _| Ok(true));
    assert_eq!(connect().unwrap(), key);
    SUSPENDED.lock().unwrap().push("key-suspend1".into());
    assert!(matches!(connect(), Err(Error::Suspended)));
    resume("key-suspend1");
    assert_eq!(connect().unwrap(), key);
}
