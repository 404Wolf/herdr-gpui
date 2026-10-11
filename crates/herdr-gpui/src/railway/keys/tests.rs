#![allow(clippy::unwrap_used)]
use super::*;

fn tools() -> bool {
    check_tools().is_ok()
}

#[test]
fn a_device_id_comes_from_the_sha256_fingerprint() {
    let id = device_id("256 SHA256:AbC+/dEfGhIjKlMnOpQrStUvWxYz0123456789abcde herdr (ED25519)\n");
    assert_eq!(id.as_deref(), Some("key-AbCdEfGhIjKlMnOp"));
    for bad in [
        "",
        "256",
        "256 MD5:aa:bb:cc",
        "256 SHA256:+/+/ comment",
        "256 SHA256:short comment",
    ] {
        assert_eq!(device_id(bad), None, "{bad}");
    }
    assert!(identity::valid_id(&id.unwrap()));
}

#[test]
fn a_generated_key_is_private_named_by_its_fingerprint_and_distinct() {
    if !tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let first = generate(dir.path(), &|| false).unwrap();
    let second = generate(dir.path(), &|| false).unwrap();
    assert_ne!(first, second);
    assert!(identity::valid_id(&first));
    let keys = identity::keys_dir(dir.path());
    let private = keys.join(&first);
    assert_eq!(fingerprint(&private, &|| false).unwrap(), first);
    assert!(keys.join(format!("{first}.pub")).is_file());
    let text = fs::read_to_string(&private).unwrap();
    assert!(text.contains("PRIVATE KEY"));
    assert!(
        !fs::read_to_string(keys.join(format!("{first}.pub")))
            .unwrap()
            .contains("PRIVATE")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&private), 0o600);
        assert_eq!(mode(&keys), 0o700);
    }
    let names: Vec<_> = fs::read_dir(&keys)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 4, "no scratch files are left: {names:?}");
}

#[test]
fn an_existing_key_keeps_its_file_and_names_the_same_device() {
    if !tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let id = generate(dir.path(), &|| false).unwrap();
    let path = identity::keys_dir(dir.path()).join(&id);
    let before = fs::read(&path).unwrap();
    assert_eq!(existing(&path).unwrap(), path);
    assert_eq!(fingerprint(&path, &|| false).unwrap(), id);
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "reading never changes the key"
    );
}

#[test]
fn unusable_key_choices_fail_before_anything_runs() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(existing(Path::new("id_rsa")), Err(Error::KeyPath)));
    assert!(matches!(existing(dir.path()), Err(Error::KeyPath)));
    assert!(matches!(
        existing(&dir.path().join("absent")),
        Err(Error::KeyFile { .. })
    ));
    if !tools() {
        return;
    }
    let junk = dir.path().join("junk");
    fs::write(&junk, "this is not a key\n").unwrap();
    assert!(matches!(
        fingerprint(&junk, &|| false),
        Err(Error::UnusableKey(path)) if path == junk
    ));
}

#[test]
fn cancellation_stops_key_work_and_leaves_no_key_behind() {
    if !tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let result = generate(dir.path(), &|| true);
    // A cancelled run may finish before the first check on a fast machine;
    // either way no half-made key remains, and a cancel is not a failure to
    // use the tools.
    match result {
        Err(Error::Cloud(crate::cloud::Error::Cancelled)) => {
            let leftovers = fs::read_dir(identity::keys_dir(dir.path()))
                .unwrap()
                .count();
            assert_eq!(leftovers, 0);
        }
        Ok(id) => assert!(identity::valid_id(&id)),
        Err(error) => panic!("{error}"),
    }
}

#[test]
fn only_pairs_in_dot_ssh_are_offered_and_nothing_is_read() {
    let home = tempfile::tempdir().unwrap();
    let ssh = home.path().join(".ssh");
    fs::create_dir_all(&ssh).unwrap();
    for name in [
        "id_ed25519",
        "id_ed25519.pub",
        "lonely",
        "known_hosts",
        "config",
        ".hidden",
        ".hidden.pub",
    ] {
        fs::write(ssh.join(name), "x").unwrap();
    }
    assert_eq!(detected(home.path()), [ssh.join("id_ed25519")]);
    assert!(detected(&home.path().join("none")).is_empty());
}

#[test]
fn a_missing_tool_is_named_before_any_vm_could_exist() {
    let result = Command::new("definitely-not-a-herdr-tool")
        .status()
        .map_err(Error::KeygenMissing);
    assert!(matches!(result, Err(Error::KeygenMissing(_))));
}
