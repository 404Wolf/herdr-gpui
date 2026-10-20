use super::*;
use std::{fs, sync::Arc};

#[test]
fn resolves_exact_id_and_refuses_ambiguity() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("projects/first");
    fs::create_dir_all(&project).unwrap();
    let path = project.join(format!("{ID}.jsonl"));
    fs::write(&path, b"").unwrap();
    assert_eq!(
        resolve(dir.path(), ID).unwrap(),
        fs::canonicalize(&path).unwrap()
    );
    assert!(matches!(
        resolve(dir.path(), "../../escape"),
        Err(Error::Identity)
    ));
    let other = dir.path().join("projects/second");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join(format!("{ID}.jsonl")), b"").unwrap();
    assert!(matches!(resolve(dir.path(), ID), Err(Error::Ambiguous)));
}

#[test]
fn cached_reads_refresh_on_append_and_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let first = message("user", "a", None, json!("first"));
    fs::write(&path, jsonl(std::slice::from_ref(&first))).unwrap();
    let mut reader = Reader::default();
    let old = reader.read(&path, ID, 1).unwrap();
    assert!(Arc::ptr_eq(&old, &reader.read(&path, ID, 1).unwrap()));
    fs::write(
        &path,
        jsonl(&[
            first,
            message("assistant", "b", Some("a"), json!("response")),
        ]),
    )
    .unwrap();
    assert_eq!(reader.read(&path, ID, 1).unwrap().turns.len(), 2);
    assert_eq!(old.turns.len(), 1);
    fs::write(&path, b"").unwrap();
    assert!(reader.read(&path, ID, 1).unwrap().turns.is_empty());
}
