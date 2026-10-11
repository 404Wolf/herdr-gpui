#![allow(clippy::unwrap_used)]
use super::*;

fn generated(id: &str) -> Record {
    Record::unattached(id, Source::Generated { path: None })
}

fn borrowed(id: &str, path: &str) -> Record {
    Record::unattached(id, Source::Borrowed { path: path.into() })
}

/// A key generated at an exact path, as a named key is.
fn named(id: &str, path: &Path) -> Record {
    Record::unattached(
        id,
        Source::Generated {
            path: Some(path.display().to_string()),
        },
    )
}

#[test]
fn records_round_trip_replace_by_id_and_stay_private() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_all(dir.path()).unwrap().is_empty());
    save(dir.path(), generated("key-aaaa")).unwrap();
    save(dir.path(), borrowed("key-bbbb", "/home/u/.ssh/id")).unwrap();
    let reached = generated("key-aaaa").reached(100, None);
    save(dir.path(), reached.clone()).unwrap();
    let saved = load_all(dir.path()).unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(load(dir.path(), "key-aaaa").unwrap(), Some(reached));
    assert_eq!(load(dir.path(), "key-zzzz").unwrap(), None);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(dir.path().join(FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "the file names private keys: {mode:o}");
    }
}

#[test]
fn damaged_or_unsafe_records_are_refused_not_guessed_at() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(FILE);
    let write = |text: &str| fs::write(&file, text).unwrap();
    for text in [
        "not json",
        r#"{"version":2,"identities":[]}"#,
        r#"{"version":1,"identities":[],"extra":1}"#,
        // An ID that could name another file.
        r#"{"version":1,"identities":[{"id":"key-../x","source":{"kind":"generated"},"attached":true,"window_ends_by":null,"claimed":false}]}"#,
        r#"{"version":1,"identities":[{"id":"x","source":{"kind":"generated"},"attached":true,"window_ends_by":null,"claimed":false}]}"#,
        // A borrowed key must be an absolute path.
        r#"{"version":1,"identities":[{"id":"key-a","source":{"kind":"borrowed","path":"id_rsa"},"attached":true,"window_ends_by":null,"claimed":false}]}"#,
    ] {
        write(text);
        assert!(read_in(dir.path()).is_err(), "{text}");
    }
    assert!(save(dir.path(), generated("key-/etc")).is_err());
    fs::write(&file, vec![b' '; LIMIT as usize + 1]).unwrap();
    assert!(matches!(read_in(dir.path()), Err(Error::Invalid(_))));
}

#[test]
fn ids_are_plain_key_prefixed_names() {
    assert!(valid_id("key-AbC123"));
    for id in [
        "", "key-", "key-a-b", "key-a.b", "key-a/b", "../key-a", "KEY-a", "key-é",
    ] {
        assert!(!valid_id(id), "{id}");
    }
}

#[test]
fn the_build_window_is_an_upper_bound_and_unknown_means_ended() {
    let fresh = generated("key-a").reached(1000, None);
    assert_eq!(fresh.window_ends_by, Some(1000 + WINDOW_SECONDS));
    assert!(fresh.attached);
    assert!(!fresh.window_ended(1000 + WINDOW_SECONDS));
    assert!(fresh.window_ended(1000 + WINDOW_SECONDS + 1));
    // Never reached, so there is no basis to call the VM current.
    assert!(generated("key-a").window_ended(0));
    let claimed = Record {
        claimed: true,
        ..fresh
    };
    assert!(
        !claimed.window_ended(u64::MAX),
        "a claimed VM has no window"
    );
    // Railway's own deadline wins over the bound, even a short one.
    assert_eq!(
        generated("key-a").reached(1000, Some(1500)).window_ends_by,
        Some(1500)
    );
    // Reaching the same VM again never grants it a longer window...
    let known = generated("key-a").reached(1000, Some(2000));
    assert_eq!(known.clone().reached(1100, None).window_ends_by, Some(2000));
    // ...but one already past says nothing about a VM set up since.
    assert_eq!(
        known.reached(5000, None).window_ends_by,
        Some(5000 + WINDOW_SECONDS)
    );
    // Saturates instead of wrapping into the past.
    assert_eq!(
        generated("key-a").reached(u64::MAX, None).window_ends_by,
        Some(u64::MAX)
    );
}

#[test]
fn only_an_unfinished_generated_key_is_waiting_for_reuse() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), borrowed("key-b", "/k")).unwrap();
    save(dir.path(), generated("key-a").reached(1, None)).unwrap();
    assert_eq!(unattached_generated(dir.path()).unwrap(), None);
    save(dir.path(), generated("key-c")).unwrap();
    assert_eq!(
        unattached_generated(dir.path()).unwrap().unwrap().id,
        "key-c"
    );
}

#[test]
fn forgetting_deletes_a_key_in_herdrs_folder_and_never_a_borrowed_one() {
    let dir = tempfile::tempdir().unwrap();
    let keys = keys_dir(dir.path());
    fs::create_dir_all(&keys).unwrap();
    fs::write(keys.join("key-a"), "private").unwrap();
    fs::write(keys.join("key-a.pub"), "public").unwrap();
    fs::write(keys.join("key-other"), "private").unwrap();
    let theirs = dir.path().join("id_ed25519");
    fs::write(&theirs, "theirs").unwrap();
    fs::write(dir.path().join("id_ed25519.pub"), "theirs").unwrap();
    save(dir.path(), generated("key-a").reached(1, None)).unwrap();
    save(dir.path(), borrowed("key-b", theirs.to_str().unwrap())).unwrap();

    forget(dir.path(), None, "key-a", false).unwrap();
    assert!(!keys.join("key-a").exists() && !keys.join("key-a.pub").exists());
    assert!(keys.join("key-other").exists(), "only its own key goes");
    assert_eq!(load(dir.path(), "key-a").unwrap(), None);

    // Not even an explicit request deletes a key the user brought.
    forget(dir.path(), None, "key-b", true).unwrap();
    assert_eq!(fs::read_to_string(&theirs).unwrap(), "theirs");
    assert!(dir.path().join("id_ed25519.pub").exists());
    assert_eq!(load(dir.path(), "key-b").unwrap(), None);
    // Forgetting what is not there is not an error.
    forget(dir.path(), None, "key-b", false).unwrap();
}

#[test]
fn a_key_generated_in_ssh_stays_unless_deleting_it_was_requested() {
    let state = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let ssh = home.path().join(".ssh");
    fs::create_dir_all(&ssh).unwrap();
    let key = ssh.join("herdr-railway");
    let public = public_path(&key);
    let write = || {
        fs::write(&key, "private").unwrap();
        fs::write(&public, "public").unwrap();
    };
    write();
    let record = named("key-a", &key).reached(1, None);
    assert!(record.kept(state.path()) && record.owned());
    save(state.path(), record.clone()).unwrap();

    forget(state.path(), Some(home.path()), "key-a", false).unwrap();
    assert!(
        key.exists() && public.exists(),
        "removing the device keeps it"
    );
    assert_eq!(load(state.path(), "key-a").unwrap(), None);

    save(state.path(), record).unwrap();
    forget(state.path(), Some(home.path()), "key-a", true).unwrap();
    assert!(
        !key.exists() && !public.exists(),
        "explicit deletion removes the pair"
    );
}

#[test]
fn a_saved_path_can_never_point_a_deletion_at_another_file() {
    let state = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".ssh")).unwrap();
    let precious = elsewhere.path().join("precious");
    fs::write(&precious, "data").unwrap();
    fs::write(public_path(&precious), "data").unwrap();
    // Outside both key folders, in a folder beside one, and with an odd name.
    let sneaky = [
        precious.clone(),
        home.path().join(".ssh/../precious"),
        home.path().join(".ssh/sub/key"),
        home.path().join(".ssh/.hidden"),
        home.path().join(".ssh/config.pub"),
        home.path().join("key"),
    ];
    for path in sneaky {
        fs::write(&path, "data").ok();
        save(state.path(), named("key-x", &path).reached(1, None)).unwrap();
        forget(state.path(), Some(home.path()), "key-x", true).unwrap();
        assert!(
            precious.exists() && public_path(&precious).exists(),
            "{}",
            path.display()
        );
    }
    assert!(home.path().join(".ssh/.hidden").exists());
    assert!(home.path().join("key").exists());
}

#[test]
fn records_from_before_named_keys_still_load_and_find_their_key() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(FILE),
        r#"{"version":1,"identities":[{"id":"key-old","source":{"kind":"generated"},"attached":true,"window_ends_by":5,"claimed":false}]}"#,
    )
    .unwrap();
    let record = load(dir.path(), "key-old").unwrap().unwrap();
    assert_eq!(record.source, Source::Generated { path: None });
    assert_eq!(record.fingerprint, None);
    assert_eq!(
        record.key_path(dir.path()),
        keys_dir(dir.path()).join("key-old")
    );
    assert!(
        !record.kept(dir.path()),
        "a legacy key is in Herdr's folder"
    );
}

#[test]
fn a_named_key_saves_its_exact_path_and_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let mut record = named("key-n", Path::new("/home/u/.ssh/my.key"));
    record.fingerprint = Some("SHA256:abcdef".into());
    save(dir.path(), record.clone()).unwrap();
    let loaded = load(dir.path(), "key-n").unwrap().unwrap();
    assert_eq!(loaded, record);
    assert_eq!(
        loaded.key_path(dir.path()),
        Path::new("/home/u/.ssh/my.key")
    );
    // A relative or control-laden saved path is refused on read.
    for path in ["relative/key", "/k\u{7}ey"] {
        let mut bad = named("key-bad", Path::new(path));
        bad.source = Source::Generated {
            path: Some(path.into()),
        };
        assert!(save(dir.path(), bad).is_err(), "{path}");
    }
}

#[test]
fn a_record_finds_its_key() {
    let dir = Path::new("/state");
    assert_eq!(
        generated("key-a").key_path(dir),
        Path::new("/state/railway-keys/key-a")
    );
    assert_eq!(
        borrowed("key-b", "/home/u/.ssh/id").key_path(dir),
        Path::new("/home/u/.ssh/id")
    );
}
