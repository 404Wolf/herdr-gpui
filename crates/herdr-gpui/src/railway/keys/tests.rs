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

/// A key directory holding no keys yet, in Herdr's own folder or a `~/.ssh`.
fn directory(root: &Path, user: bool) -> KeyDirectory {
    key_directory(user.then_some(root), root)
}

#[test]
fn key_names_are_one_safe_file_name() {
    for good in ["herdr-railway", "a", "Key_1.work", "x.y.z", &"k".repeat(64)] {
        assert!(valid_name(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        ".hidden",
        "-option",
        "a/b",
        "../escape",
        "..",
        ".",
        "a\\b",
        "name.pub",
        "with space",
        "tab\t",
        "nul\0",
        "é",
        &"k".repeat(65),
    ] {
        assert!(matches!(valid_name(bad), Err(Error::KeyName)), "{bad:?}");
    }
}

#[test]
fn new_keys_go_to_dot_ssh_when_it_exists_else_to_herdrs_folder() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    // No ~/.ssh: Herdr's folder, which is not the user's.
    let fallback = key_directory(Some(root.path()), &state);
    assert_eq!(fallback.path, identity::keys_dir(&state));
    assert!(!fallback.user);
    assert!(!key_directory(None, &state).user);
    // A file named .ssh is not a directory.
    fs::write(root.path().join(".ssh"), "file").unwrap();
    assert!(!key_directory(Some(root.path()), &state).user);
    fs::remove_file(root.path().join(".ssh")).unwrap();
    fs::create_dir(root.path().join(".ssh")).unwrap();
    let ssh = key_directory(Some(root.path()), &state);
    assert_eq!(ssh.path, root.path().join(".ssh"));
    assert!(ssh.user);
}

#[test]
fn a_named_key_is_created_private_where_asked_with_nothing_left_over() {
    if !tools() {
        return;
    }
    for user in [false, true] {
        let root = tempfile::tempdir().unwrap();
        if user {
            fs::create_dir(root.path().join(".ssh")).unwrap();
        }
        let directory = directory(root.path(), user);
        let made = generate(&directory, "herdr-railway", &|| false).unwrap();
        assert_eq!(made.path, directory.path.join("herdr-railway"));
        assert!(made.fingerprint.starts_with("SHA256:"));
        // The fingerprint saved is the one the file has.
        assert_eq!(
            fingerprint_text(&made.path, &|| false).unwrap(),
            made.fingerprint
        );
        let public = identity::public_path(&made.path);
        assert!(
            fs::read_to_string(&made.path)
                .unwrap()
                .contains("PRIVATE KEY")
        );
        assert!(!fs::read_to_string(&public).unwrap().contains("PRIVATE"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&made.path), 0o600);
            if !user {
                assert_eq!(
                    mode(&directory.path),
                    0o700,
                    "Herdr's own folder is private"
                );
            }
        }
        let names: Vec<_> = fs::read_dir(&directory.path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2, "no scratch files are left: {names:?}");
    }
}

#[test]
fn generating_never_replaces_anything_that_has_the_name() {
    if !tools() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".ssh")).unwrap();
    let directory = directory(root.path(), true);
    let target = directory.path.join("taken");
    let public = identity::public_path(&target);
    // Each of these must stop the run and leave the existing file as it was.
    #[cfg(unix)]
    let victim = root.path().join("victim");
    for create in [0, 1, 2] {
        match create {
            0 => fs::write(&target, "mine").unwrap(),
            1 => {
                fs::remove_file(&target).unwrap();
                fs::write(&public, "mine").unwrap();
            }
            _ => {
                fs::remove_file(&public).unwrap();
                // A dangling link is a name already taken, and must not be
                // followed into the file it points at.
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&victim, &target).unwrap();
                }
                #[cfg(not(unix))]
                continue;
            }
        }
        let result = generate(&directory, "taken", &|| false);
        assert!(
            matches!(&result, Err(Error::KeyExists(path)) if *path == target),
            "{result:?}"
        );
        #[cfg(unix)]
        assert!(!victim.exists(), "nothing was written through the link");
        let leftovers = fs::read_dir(&directory.path).unwrap().count();
        assert_eq!(leftovers, 1, "no scratch files after a refusal");
    }
    assert_eq!(fs::read_to_string(&public).unwrap_or_default(), "");
}

#[test]
fn generate_refuses_an_unsafe_name_before_touching_the_disk() {
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), false);
    for bad in ["../escape", "a/b", ".hidden", "x.pub", ""] {
        assert!(matches!(
            generate(&directory, bad, &|| false),
            Err(Error::KeyName)
        ));
    }
    assert!(
        !directory.path.exists(),
        "not even the folder is created for a bad name"
    );
}

#[test]
fn an_existing_key_keeps_its_file_and_names_the_same_device() {
    if !tools() {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), false);
    let made = generate(&directory, "mine", &|| false).unwrap();
    let before = fs::read(&made.path).unwrap();
    assert_eq!(existing(&made.path).unwrap(), made.path);
    let id = fingerprint(&made.path, &|| false).unwrap();
    assert_eq!(Some(id), id_of(&made.fingerprint));
    assert_eq!(
        fs::read(&made.path).unwrap(),
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
    let root = tempfile::tempdir().unwrap();
    let directory = directory(root.path(), false);
    let result = generate(&directory, "cancelled", &|| true);
    // A cancelled run may finish before the first check on a fast machine;
    // either way no half-made key remains, and a cancel is not a failure to
    // use the tools.
    match result {
        Err(Error::Cloud(crate::cloud::Error::Cancelled)) => {
            let leftovers = fs::read_dir(&directory.path).unwrap().count();
            assert_eq!(leftovers, 0);
        }
        Ok(made) => assert!(made.path.exists()),
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
