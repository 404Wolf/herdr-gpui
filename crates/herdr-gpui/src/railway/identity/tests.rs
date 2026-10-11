#![allow(clippy::unwrap_used)]
use super::*;

fn record(id: &str, path: &str) -> Record {
    Record::unattached(id, path)
}

#[test]
fn identities_round_trip_and_forgetting_one_leaves_its_key_files_alone() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("id");
    fs::write(&key, "private").unwrap();
    fs::write(public_path(&key), "public").unwrap();
    let saved = record("key-a", &key.display().to_string());
    save(dir.path(), saved.clone()).unwrap();
    save(dir.path(), record("key-b", "/elsewhere/id")).unwrap();
    assert_eq!(load(dir.path(), "key-a").unwrap(), Some(saved.clone()));
    assert_eq!(pending(dir.path()).unwrap().unwrap().id, "key-a");

    save(
        dir.path(),
        Record {
            attached: true,
            ..saved
        },
    )
    .unwrap();
    assert_eq!(pending(dir.path()).unwrap().unwrap().id, "key-b");

    forget(dir.path(), "key-a").unwrap();
    assert_eq!(load(dir.path(), "key-a").unwrap(), None);
    assert_eq!(fs::read_to_string(&key).unwrap(), "private");
    assert_eq!(fs::read_to_string(public_path(&key)).unwrap(), "public");
}

#[test]
fn restore_puts_an_identity_back_exactly_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let original = record("key-a", "/k/a");
    save(dir.path(), original.clone()).unwrap();
    save(
        dir.path(),
        Record {
            attached: true,
            ..original.clone()
        },
    )
    .unwrap();
    restore(dir.path(), "key-a", Some(original.clone())).unwrap();
    assert_eq!(load(dir.path(), "key-a").unwrap(), Some(original));
    restore(dir.path(), "key-a", None).unwrap();
    assert!(load_all(dir.path()).unwrap().is_empty());
}

#[test]
fn a_document_that_could_not_be_read_back_never_replaces_one_that_can() {
    let dir = tempfile::tempdir().unwrap();
    save(dir.path(), record("key-a", "/k/a")).unwrap();
    // Records that are individually valid but together exceed the read limit.
    let long = format!("/{}", "x".repeat(PATH_LIMIT - 1));
    let many: Vec<Record> = (0..MAX_RECORDS)
        .map(|index| record(&format!("key-{index}"), &long))
        .collect();
    assert!(matches!(write_in(dir.path(), many), Err(Error::Invalid(_))));
    assert!(matches!(
        write_in(dir.path(), vec![record("key-a", "relative")]),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        write_in(dir.path(), vec![record("../x", "/k")]),
        Err(Error::Invalid(_))
    ));
    assert_eq!(load_all(dir.path()).unwrap(), [record("key-a", "/k/a")]);
}

#[test]
fn an_identity_file_from_before_this_format_holds_no_identity() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join(FILE),
        r#"{"version":1,"identities":[{"id":"key-a","source":{"kind":"borrowed","path":"/k"},"attached":true,"window_ends_by":1,"claimed":false}]}"#,
    )
    .unwrap();
    assert!(load_all(dir.path()).unwrap().is_empty());
}
