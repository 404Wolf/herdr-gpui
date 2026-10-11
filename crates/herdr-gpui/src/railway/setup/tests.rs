#![allow(clippy::unwrap_used)]
use super::{super::WINDOW_SECONDS, *};
use std::{cell::RefCell, fs, process::Command};

fn tools() -> bool {
    keys::check_tools().is_ok()
}

fn generated(id: &str) -> Record {
    Record {
        id: id.into(),
        source: Source::Generated,
        attached: false,
        window_ends_by: None,
        claimed: false,
    }
}

#[test]
fn generating_creates_one_owned_key_and_a_retry_reuses_it() {
    if !tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let first = resolve(dir.path(), Choice::Generate, &|| false).unwrap();
    assert_eq!(first.source, Source::Generated);
    assert!(!first.attached, "not attached until setup succeeds");
    assert!(first.key_path(dir.path()).is_file());
    // A failed or cancelled setup spent a VM; the retry reaches the same one.
    let retry = resolve(dir.path(), Choice::Generate, &|| false).unwrap();
    assert_eq!(retry, first);
    assert_eq!(identity::load_all(dir.path()).unwrap().len(), 1);
    // Once a device uses it, the next request is for a new key and a new VM.
    identity::save(dir.path(), first.clone().reached(1, None)).unwrap();
    let next = resolve(dir.path(), Choice::Generate, &|| false).unwrap();
    assert_ne!(next.id, first.id);
    assert_eq!(identity::load_all(dir.path()).unwrap().len(), 2);
}

#[test]
fn an_existing_key_is_borrowed_by_path_and_never_copied() {
    if !tools() {
        return;
    }
    let state = tempfile::tempdir().unwrap();
    let theirs = tempfile::tempdir().unwrap();
    let id = keys::generate(theirs.path(), &|| false).unwrap();
    let key = identity::keys_dir(theirs.path()).join(&id);
    let before = fs::read(&key).unwrap();
    let record = resolve(state.path(), Choice::Existing(key.clone()), &|| false).unwrap();
    assert_eq!(record.id, id);
    assert_eq!(
        record.source,
        Source::Borrowed {
            path: key.display().to_string()
        }
    );
    assert!(!record.owned());
    assert_eq!(fs::read(&key).unwrap(), before);
    assert!(
        !identity::keys_dir(state.path()).exists(),
        "a borrowed key is not copied into Herdr's state"
    );
    assert!(identity::load_all(state.path()).unwrap().is_empty());
}

#[test]
fn pointing_at_a_known_key_keeps_who_owns_it() {
    if !tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ours = resolve(dir.path(), Choice::Generate, &|| false).unwrap();
    let path = ours.key_path(dir.path());
    let again = resolve(dir.path(), Choice::Existing(path), &|| false).unwrap();
    assert_eq!(
        again.source,
        Source::Generated,
        "never downgraded to borrowed"
    );

    // A borrowed key that moved follows the path chosen now.
    let other = tempfile::tempdir().unwrap();
    let id = keys::generate(other.path(), &|| false).unwrap();
    let old = identity::keys_dir(other.path()).join(&id);
    let moved = other.path().join("moved");
    fs::copy(&old, &moved).unwrap();
    fs::copy(old.with_extension("pub"), other.path().join("moved.pub")).unwrap();
    identity::save(
        dir.path(),
        Record {
            id: id.clone(),
            source: Source::Borrowed {
                path: "/gone/key".into(),
            },
            attached: true,
            window_ends_by: Some(5),
            claimed: true,
        },
    )
    .unwrap();
    let record = resolve(dir.path(), Choice::Existing(moved.clone()), &|| false).unwrap();
    assert_eq!(
        record.source,
        Source::Borrowed {
            path: moved.display().to_string()
        }
    );
    assert!(record.claimed && record.window_ends_by == Some(5));
}

#[test]
fn a_saved_choice_never_generates_and_a_bad_path_never_runs_anything() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        resolve(dir.path(), Choice::Saved("key-a".into()), &|| false),
        Err(Error::NoIdentity)
    ));
    assert!(matches!(
        resolve(dir.path(), Choice::Existing("relative".into()), &|| false),
        Err(Error::KeyPath)
    ));
    assert!(matches!(
        resolve(
            dir.path(),
            Choice::Existing(dir.path().join("absent")),
            &|| false
        ),
        Err(Error::KeyFile { .. })
    ));
    assert!(!identity::keys_dir(dir.path()).exists());
    identity::save(dir.path(), generated("key-a")).unwrap();
    let saved = resolve(dir.path(), Choice::Saved("key-a".into()), &|| false).unwrap();
    assert_eq!(saved.id, "key-a");
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

fn herdr_installed(home: &Path) -> bool {
    home.join("herdr").exists()
}

#[test]
fn herdr_is_installed_before_a_session_only_when_approved() {
    let steps = RefCell::new(Vec::new());
    let report = |step: cloud::Step| steps.borrow_mut().push(step);

    let home = tempfile::tempdir().unwrap();
    let declined = ensure_herdr(vm(home.path(), true), false, &|| false, &report);
    assert!(matches!(
        declined,
        Err(Error::Cloud(cloud::Error::Install(text))) if text.contains("allow installing")
    ));
    assert!(!herdr_installed(home.path()), "nothing was installed");
    assert!(steps.borrow().is_empty());

    ensure_herdr(vm(home.path(), true), true, &|| false, &report).unwrap();
    assert!(herdr_installed(home.path()));
    assert_eq!(*steps.borrow(), [cloud::Step::InstallingHerdr]);

    // Already there: the probe passes and the installer is not run again.
    steps.borrow_mut().clear();
    ensure_herdr(vm(home.path(), false), true, &|| false, &report).unwrap();
    assert!(steps.borrow().is_empty());
}

#[test]
fn an_installer_that_leaves_no_herdr_is_an_error_not_a_ready_device() {
    let home = tempfile::tempdir().unwrap();
    let result = ensure_herdr(vm(home.path(), false), true, &|| false, &|_| {});
    assert!(matches!(
        result,
        Err(Error::Cloud(cloud::Error::Install(text))) if text.contains("expected paths")
    ));
}

#[test]
fn an_unreachable_vm_is_a_failure_with_its_output() {
    let failing = || {
        let mut command = Command::new("/bin/sh");
        command.args([
            "-c",
            "echo 'Permission denied (publickey)' >&2; exit 255",
            "sh",
        ]);
        Ok(command)
    };
    let result = ensure_herdr(failing, true, &|| false, &|_| {});
    assert!(matches!(
        result,
        Err(Error::Cloud(cloud::Error::Install(text))) if text.contains("Permission denied")
    ));
}

#[test]
fn cancelling_stops_the_remote_command() {
    let slow = || {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 30", "sh"]);
        Ok(command)
    };
    let started = std::time::Instant::now();
    let result = ensure_herdr(slow, true, &|| true, &|_| {});
    assert!(matches!(result, Err(Error::Cloud(cloud::Error::Cancelled))));
    assert!(started.elapsed() < Duration::from_secs(10));
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

use std::time::Duration;

#[test]
fn a_ready_vm_becomes_a_device_with_its_identity_attached() {
    let dir = tempfile::tempdir().unwrap();
    identity::save(dir.path(), generated("key-a")).unwrap();
    let device = attach(
        dir.path(),
        generated("key-a"),
        None,
        || Ok(Vec::new()),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(device.provider, CloudProvider::Railway);
    assert_eq!(device.id, "key-a");
    assert_eq!(device.account, ACCOUNT);
    assert_eq!(device.session, "default");
    assert!(device.enabled && device.label.starts_with("railway-"));
    let record = identity::load(dir.path(), "key-a").unwrap().unwrap();
    assert!(record.attached);
    let ends = record.window_ends_by.unwrap();
    assert!(ends > identity::now() + 3000 && ends <= identity::now() + WINDOW_SECONDS);
    assert!(unattached(dir.path()).is_none());
}

fn unattached(dir: &Path) -> Option<Record> {
    identity::unattached_generated(dir).unwrap()
}

#[test]
fn setting_a_device_up_again_keeps_its_name_and_other_devices_alone() {
    let dir = tempfile::tempdir().unwrap();
    let existing = vec![saved("key-other", "Other"), saved("key-a", "My VM")];
    let device = attach(
        dir.path(),
        generated("key-a"),
        None,
        || Ok(existing),
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(device.label, "My VM");
    assert_eq!(device.machine, "railway-old-name");
}

#[test]
fn a_device_that_cannot_be_saved_leaves_a_generated_key_reusable() {
    let dir = tempfile::tempdir().unwrap();
    let result = attach(
        dir.path(),
        generated("key-a"),
        None,
        || Ok(Vec::new()),
        |_| Err(cloud::Error::StateDirectory),
    );
    assert!(matches!(
        result,
        Err(Error::Cloud(cloud::Error::StateDirectory))
    ));
    let record = identity::load(dir.path(), "key-a").unwrap().unwrap();
    assert!(!record.attached, "the key is not attached to any device");
    assert_eq!(unattached(dir.path()).unwrap().id, "key-a");
}

#[test]
fn typed_paths_expand_only_a_leading_home_shortcut() {
    assert_eq!(expand("/abs/key"), Path::new("/abs/key"));
    assert_eq!(expand("relative/key"), Path::new("relative/key"));
    assert_eq!(expand("a/~/b"), Path::new("a/~/b"));
    if let Some(home) = std::env::var_os("HOME") {
        assert_eq!(expand("~/.ssh/id"), Path::new(&home).join(".ssh/id"));
    }
}

#[test]
fn railways_own_deadline_is_kept_when_it_gave_one() {
    let dir = tempfile::tempdir().unwrap();
    let deadline = identity::now() + 1200;
    attach(
        dir.path(),
        generated("key-a"),
        Some(deadline),
        || Ok(Vec::new()),
        |_| Ok(()),
    )
    .unwrap();
    let record = identity::load(dir.path(), "key-a").unwrap().unwrap();
    assert_eq!(record.window_ends_by, Some(deadline));
}

/// `ssh railway.new true` as a local shell that ends the way the script says.
fn railway(script: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", script, "sh"]);
    command
}

fn manifest_line(expires_at: u64) -> String {
    let at = chrono::DateTime::from_timestamp(expires_at as i64, 0).unwrap();
    format!(
        r#"{{"status":"trial_ready","human_claim_url":"https://claim.example/secret","build_expires_at":"{}"}}"#,
        at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    )
}

#[test]
fn a_refused_first_connection_names_the_command_the_user_must_run() {
    let key = Path::new("/state/railway-keys/key-a");
    let refused = reach(
        railway(
            r#"echo '{"status":"refused","description":"Headless terminals are not allowed"}'; exit 13"#,
        ),
        key,
        &|| false,
    );
    let Err(Error::FirstUseRequired { command }) = refused else {
        panic!("a refusal asks for an interactive first connection");
    };
    assert_eq!(
        command,
        "ssh -F /dev/null -i '/state/railway-keys/key-a' -o IdentitiesOnly=yes railway.new"
    );
}

#[test]
fn the_first_use_command_quotes_any_path_for_a_posix_shell() {
    assert_eq!(
        first_use_command(Path::new("/home/o'neil/id key")),
        r"ssh -F /dev/null -i '/home/o'\''neil/id key' -o IdentitiesOnly=yes railway.new"
    );
    // The shell sees one argument whatever the name holds.
    for path in ["/tmp/a b;touch x $(y)", "/tmp/it's `x`"] {
        let command = first_use_command(Path::new(path));
        let script = format!("set -- {}; printf %s \"$4\"", &command["ssh ".len()..]);
        let output = Command::new("/bin/sh")
            .args(["-c", &script])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout), path);
    }
}

#[test]
fn a_reachable_vm_reports_its_deadline_and_an_ended_one_is_refused() {
    let key = Path::new("/k");
    let live = identity::now() + 1800;
    let script = format!("echo '{}' >&2; exit 0", manifest_line(live));
    assert_eq!(reach(railway(&script), key, &|| false).unwrap(), Some(live));
    // No manifest: still reachable, with no deadline to record.
    assert_eq!(reach(railway("exit 0"), key, &|| false).unwrap(), None);
    let ended = format!("echo '{}' >&2", manifest_line(identity::now() - 60));
    assert!(matches!(
        reach(railway(&ended), key, &|| false),
        Err(Error::Expired)
    ));
}

#[test]
fn other_failures_hide_links_and_never_pose_as_a_first_use_request() {
    let script = format!(
        "echo '{}' >&2; echo 'Permission denied (publickey)' >&2; exit 255",
        manifest_line(identity::now() + 60)
    );
    let result = reach(railway(&script), Path::new("/k"), &|| false);
    let Err(Error::Unreachable(text)) = result else {
        panic!("a failed connection is not a first-use refusal");
    };
    assert!(text.contains("Permission denied"), "{text}");
    assert!(
        !text.contains("claim.example") && !text.contains("secret"),
        "{text}"
    );
}

#[test]
fn simultaneous_generate_requests_share_one_key() {
    if !tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ids: Vec<_> = std::thread::scope(|scope| {
        (0..4)
            .map(|_| scope.spawn(|| resolve(dir.path(), Choice::Generate, &|| false).unwrap().id))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    assert!(ids.iter().all(|id| *id == ids[0]), "{ids:?}");
    assert_eq!(identity::load_all(dir.path()).unwrap().len(), 1);
    let files = fs::read_dir(identity::keys_dir(dir.path()))
        .unwrap()
        .count();
    assert_eq!(files, 2, "one private and one public half");
}

#[test]
fn a_generated_key_that_cannot_be_recorded_is_not_left_behind() {
    if !tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // The record file is full, so saving the new key's record must fail.
    for index in 0..64 {
        identity::save(
            dir.path(),
            Record {
                id: format!("key-full{index}"),
                source: Source::Borrowed {
                    path: format!("/keys/{index}"),
                },
                attached: true,
                window_ends_by: None,
                claimed: true,
            },
        )
        .unwrap();
    }
    assert!(resolve(dir.path(), Choice::Generate, &|| false).is_err());
    let leftovers = fs::read_dir(identity::keys_dir(dir.path()))
        .unwrap()
        .count();
    assert_eq!(leftovers, 0, "no private key without a record");
}

#[test]
fn setup_refuses_a_key_file_that_is_no_longer_the_devices_key() {
    if !tools() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let first = keys::generate(dir.path(), &|| false).unwrap();
    let second = keys::generate(dir.path(), &|| false).unwrap();
    // The device was set up with `first`, but its saved path now holds `second`.
    let swapped = identity::keys_dir(dir.path()).join(&second);
    identity::save(
        dir.path(),
        Record {
            id: first.clone(),
            source: Source::Borrowed {
                path: swapped.display().to_string(),
            },
            attached: true,
            window_ends_by: Some(u64::MAX),
            claimed: false,
        },
    )
    .unwrap();
    let request = AddRequest {
        choice: Choice::Saved(first),
        install: true,
    };
    let result = add_device_in(dir.path(), request, &|| false, &|_| {});
    assert!(matches!(result, Err(Error::KeyChanged(path)) if path == swapped));
}
