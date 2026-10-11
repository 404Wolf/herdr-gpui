#![allow(clippy::unwrap_used)]
use super::*;
use std::{cell::RefCell, fs, process::Command};

fn tools() -> bool {
    keys::check_tools().is_ok()
}

/// A home directory, with a `~/.ssh` unless `ssh` is false.
fn home(ssh: bool) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    if ssh {
        fs::create_dir(home.path().join(".ssh")).unwrap();
    }
    home
}

/// A key pair of the user's own, outside Herdr's care, in `dir`.
pub(super) fn their_key(dir: &Path, name: &str) -> (PathBuf, String) {
    let key = dir.join(name);
    let made = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&key)
        .status();
    assert!(made.is_ok_and(|status| status.success()), "ssh-keygen");
    let id = keys::fingerprint(&key, &|| false).unwrap();
    (key, id)
}

#[test]
fn a_named_key_goes_in_ssh_else_herdrs_folder_and_is_recorded_for_the_device() {
    if !tools() {
        return;
    }
    for ssh in [true, false] {
        let state = tempfile::tempdir().unwrap();
        let home = home(ssh);
        let path =
            generate_key_in(state.path(), Some(home.path()), "herdr-railway", &|| false).unwrap();
        let expected = if ssh {
            home.path().join(".ssh/herdr-railway")
        } else {
            identity::keys_dir(state.path()).join("herdr-railway")
        };
        assert_eq!(path, expected);
        let record = resolve(state.path(), Choice::Generated, &|| false).unwrap();
        assert_eq!(record.key_path(), path);
        assert!(!record.attached, "not attached until setup succeeds");
        assert_eq!(
            keys::fingerprint_text(&path, &|| false).unwrap(),
            record.fingerprint.clone().unwrap()
        );
        assert_eq!(keys::fingerprint(&path, &|| false).unwrap(), record.id);
        // Making another waits until a device has used this one.
        let waiting =
            generate_key_in(state.path(), Some(home.path()), "second", &|| false).unwrap_err();
        assert!(matches!(waiting, Error::KeyWaiting(waiting) if waiting == path));
    }
}

#[test]
fn a_name_that_is_taken_or_unsafe_creates_nothing_and_replaces_nothing() {
    if !tools() {
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let home = home(true);
    let existing = home.path().join(".ssh/id_work");
    fs::write(&existing, "the user's own key").unwrap();
    let error = generate_key_in(state.path(), Some(home.path()), "id_work", &|| false);
    assert!(matches!(error, Err(Error::KeyExists(path)) if path == existing));
    for name in ["../x", "a/b", ".hidden"] {
        let error = generate_key_in(state.path(), Some(home.path()), name, &|| false);
        assert!(matches!(error, Err(Error::KeyName)), "{name}: {error:?}");
    }
    assert_eq!(fs::read_to_string(&existing).unwrap(), "the user's own key");
    assert_eq!(fs::read_dir(home.path().join(".ssh")).unwrap().count(), 1);
    assert!(identity::load_all(state.path()).unwrap().is_empty());
}

#[test]
fn simultaneous_generate_requests_make_one_key() {
    if !tools() {
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let home = home(true);
    let results: Vec<_> = std::thread::scope(|scope| {
        (0..4)
            .map(|index| {
                let (state, home) = (state.path(), home.path());
                scope.spawn(move || {
                    generate_key_in(state, Some(home), &format!("key{index}"), &|| false)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    let made = results.iter().filter(|result| result.is_ok()).count();
    assert_eq!(made, 1, "{results:?}");
    assert_eq!(identity::load_all(state.path()).unwrap().len(), 1);
    assert_eq!(fs::read_dir(home.path().join(".ssh")).unwrap().count(), 2);
}

#[test]
fn an_existing_key_is_used_by_path_and_never_copied_or_changed() {
    if !tools() {
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let theirs = tempfile::tempdir().unwrap();
    let (key, id) = their_key(theirs.path(), "theirs");
    let before = fs::read(&key).unwrap();
    let record = resolve(state.path(), Choice::Existing(key.clone()), &|| false).unwrap();
    assert_eq!(
        (record.id.as_str(), record.key_path()),
        (id.as_str(), key.clone())
    );
    assert_eq!(fs::read(&key).unwrap(), before);
    assert!(!identity::keys_dir(state.path()).exists());
    assert!(identity::load_all(state.path()).unwrap().is_empty());
    // A relative path or a missing file never runs anything.
    assert!(matches!(
        resolve(state.path(), Choice::Existing("relative".into()), &|| false),
        Err(Error::KeyPath)
    ));
    assert!(matches!(
        resolve(
            state.path(),
            Choice::Existing(theirs.path().join("absent")),
            &|| false
        ),
        Err(Error::KeyFile { .. })
    ));
}

#[test]
fn setup_refuses_a_generated_key_file_that_has_been_replaced() {
    if !tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let home = home(true);
    let path = generate_key_in(dir.path(), Some(home.path()), "mine", &|| false).unwrap();
    let (other, _) = their_key(home.path(), "other");
    fs::copy(&other, &path).unwrap();
    fs::copy(identity::public_path(&other), identity::public_path(&path)).unwrap();
    let request = AddRequest {
        choice: Choice::Generated,
        install: true,
    };
    let result = add_device_in(
        dir.path(),
        request,
        &|| false,
        &|_| {},
        || Ok(Vec::new()),
        |_| Ok(()),
    );
    assert!(matches!(result, Err(Error::KeyChanged(changed)) if changed == path));
}

/// Stand in for `ssh railway.new` without a Herdr probe that depends on this
/// host: the appended remote command is the installer's probe or its installer
/// (told apart by the URL only the installer contains), answered against a
/// marker file. `installs` says whether "installing" really leaves Herdr.
/// The real scripts are covered by `cloud::install`'s own tests.
fn vm(home: &Path, installs: bool) -> impl Fn() -> Result<Command> {
    let home = home.to_owned();
    let script = format!(
        r#"case "$1" in
*herdr.dev/install.sh*) {install} ;;
*) [ -e "$HOME/herdr" ] && exit 0; exit 3 ;;
esac"#,
        install = if installs {
            r#"touch "$HOME/herdr"; exit 0"#
        } else {
            "exit 0"
        },
    );
    move || {
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", &script, "sh"])
            .env_clear()
            .env("HOME", &home)
            .env("PATH", "/usr/bin:/bin");
        Ok(command)
    }
}

#[test]
fn herdr_is_installed_before_a_session_only_when_approved_and_only_if_it_took() {
    let steps = RefCell::new(Vec::new());
    let report = |step: cloud::Step| steps.borrow_mut().push(step);
    let home = tempfile::tempdir().unwrap();
    let installed = || home.path().join("herdr").exists();

    let declined = ensure_herdr(vm(home.path(), true), false, &|| false, &report);
    assert!(matches!(
        declined,
        Err(Error::Cloud(cloud::Error::Install(text))) if text.contains("allow installing")
    ));
    assert!(!installed() && steps.borrow().is_empty());

    // An installer that leaves no Herdr is an error, not a ready device.
    let failed = ensure_herdr(vm(home.path(), false), true, &|| false, &|_| {});
    assert!(matches!(
        failed,
        Err(Error::Cloud(cloud::Error::Install(text))) if text.contains("expected paths")
    ));

    ensure_herdr(vm(home.path(), true), true, &|| false, &report).unwrap();
    assert!(installed());
    assert_eq!(*steps.borrow(), [cloud::Step::InstallingHerdr]);

    // Already there: the probe passes and the installer is not run again.
    steps.borrow_mut().clear();
    ensure_herdr(vm(home.path(), false), true, &|| false, &report).unwrap();
    assert!(steps.borrow().is_empty());
}

fn saved(id: &str, label: &str) -> SavedDevice {
    SavedDevice {
        provider: CloudProvider::Railway,
        id: id.into(),
        label: label.into(),
        account: ACCOUNT.into(),
        machine: "railway-old-name".into(),
        session: SESSION.into(),
        enabled: true,
    }
}

#[test]
fn a_ready_vm_becomes_a_device_and_setting_it_up_again_keeps_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let record = Record::unattached("key-a", "/k/a");
    identity::save(dir.path(), record.clone()).unwrap();
    let existing = vec![saved("key-other", "Other"), saved("key-a", "My VM")];
    let device = attach(dir.path(), record.clone(), || Ok(existing), |_| Ok(())).unwrap();
    assert_eq!(
        (device.label.as_str(), device.machine.as_str()),
        ("My VM", "railway-old-name")
    );
    assert_eq!(
        (device.provider, device.account.as_str()),
        (CloudProvider::Railway, ACCOUNT)
    );
    assert!(
        identity::load(dir.path(), "key-a")
            .unwrap()
            .unwrap()
            .attached
    );

    let fresh = attach(
        dir.path(),
        Record::unattached("key-b", "/k/b"),
        || Ok(Vec::new()),
        |_| Ok(()),
    )
    .unwrap();
    assert!(fresh.enabled && fresh.label.starts_with("railway-"));
}

#[test]
fn a_failure_while_attaching_leaves_the_identity_exactly_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    fn broken<T>() -> cloud::Result<T> {
        Err(cloud::Error::StateDirectory)
    }
    // A generated key waiting for its first device stays waiting.
    let waiting = Record::unattached("key-a", "/k/a");
    identity::save(dir.path(), waiting.clone()).unwrap();
    let result = attach(dir.path(), waiting.clone(), || Ok(Vec::new()), |_| broken());
    assert!(matches!(
        result,
        Err(Error::Cloud(cloud::Error::StateDirectory))
    ));
    assert_eq!(
        identity::load(dir.path(), "key-a").unwrap(),
        Some(waiting.clone())
    );
    // A catalog that cannot be read changes nothing either.
    assert!(attach(dir.path(), waiting.clone(), broken, |_| Ok(())).is_err());
    assert_eq!(identity::load(dir.path(), "key-a").unwrap(), Some(waiting));
    // A key no one had recorded is not left recorded, and one already
    // attached to a device stays attached.
    let new = Record::unattached("key-b", "/k/b");
    assert!(attach(dir.path(), new, || Ok(Vec::new()), |_| broken()).is_err());
    assert_eq!(identity::load(dir.path(), "key-b").unwrap(), None);
    let attached = Record {
        attached: true,
        ..Record::unattached("key-c", "/k/c")
    };
    identity::save(dir.path(), attached.clone()).unwrap();
    assert!(
        attach(
            dir.path(),
            attached.clone(),
            || Ok(Vec::new()),
            |_| broken()
        )
        .is_err()
    );
    assert_eq!(identity::load(dir.path(), "key-c").unwrap(), Some(attached));
}

#[test]
fn the_overview_says_where_a_key_will_go_and_which_key_a_device_uses() {
    if !tools() {
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let with_ssh = home(true);
    let overview = overview_in(state.path(), Some(home(false).path()), Vec::new()).unwrap();
    assert!(!overview.new_key_directory.user);
    assert!(overview.pending_key.is_none() && overview.devices.is_empty());

    let path = generate_key_in(
        state.path(),
        Some(with_ssh.path()),
        "herdr-railway",
        &|| false,
    )
    .unwrap();
    let overview = overview_in(state.path(), Some(with_ssh.path()), Vec::new()).unwrap();
    assert!(overview.new_key_directory.user);
    let pending = overview.pending_key.unwrap();
    assert_eq!(pending.path, path);
    assert!(pending.fingerprint.unwrap().starts_with("SHA256:"));

    let record = identity::pending(state.path()).unwrap().unwrap();
    identity::save(
        state.path(),
        Record {
            attached: true,
            ..record.clone()
        },
    )
    .unwrap();
    let overview = overview_in(
        state.path(),
        Some(with_ssh.path()),
        vec![saved(&record.id, "Mine")],
    )
    .unwrap();
    assert!(overview.pending_key.is_none());
    assert!(overview.devices[0].key_present);
    assert_eq!(
        overview.devices[0].key_path.as_deref(),
        Some(path.as_path())
    );
}

#[test]
fn typed_paths_expand_only_a_leading_home_shortcut() {
    assert_eq!(expand("/abs/key"), Path::new("/abs/key"));
    assert_eq!(expand("a/~/b"), Path::new("a/~/b"));
    if let Some(home) = std::env::var_os("HOME") {
        assert_eq!(expand("~/.ssh/id"), Path::new(&home).join(".ssh/id"));
    }
}
