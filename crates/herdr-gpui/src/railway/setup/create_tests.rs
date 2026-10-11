//! Setup against a stand-in for Railway: a headless probe is refused until a
//! session with an 80x32 terminal has run, as with the real service.
#![allow(clippy::unwrap_used)]
use super::{tests::their_key, *};
use crate::railway::ssh::stand_in;
use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// Logs what each run was (`@LOG@`) and keeps whether a VM exists in `@STATE@`.
/// A terminal run (`-tt`) does what `terminal` says; headless runs are refused
/// with exit 13 until the VM exists.
fn railway(terminal: &str) -> String {
    r#"mode=headless
last=
for argument in "$@"; do
    [ "$argument" = -tt ] && mode=terminal
    last=$argument
done
if [ $mode = terminal ]; then
@TERMINAL@
fi
if [ ! -e '@STATE@/vm' ]; then
    echo refused >> '@LOG@'
    echo '{"status":"refused","description":"Headless terminals are not allowed"}'
    exit 13
fi
case "$last" in
    *'exit 3'*) echo probe >> '@LOG@' ;;
    herdr-railway) echo reach >> '@LOG@' ;;
    *) echo "reach with a command" >> '@LOG@' ;;
esac
exit 0
"#
    .replace("@TERMINAL@", terminal)
}

/// A terminal run that creates the VM only when it has a terminal of 80x32.
const CREATES: &str = r#"    if [ -t 0 ] && [ "$(stty size 2>/dev/null)" = "32 80" ]; then
        echo "terminal 32 80" >> '@LOG@'
        : > '@STATE@/vm'
        exit 0
    fi
    echo "terminal wrong" >> '@LOG@'
    exit 13"#;

struct Fixture {
    dir: tempfile::TempDir,
    _keys: tempfile::TempDir,
    key: PathBuf,
    id: String,
}

fn fixture() -> Option<Fixture> {
    keys::check_tools().ok()?;
    let keys = tempfile::tempdir().unwrap();
    let (key, id) = their_key(keys.path(), "theirs");
    Some(Fixture {
        dir: tempfile::tempdir().unwrap(),
        _keys: keys,
        key,
        id,
    })
}

fn run(
    fixture: &Fixture,
    cancelled: &dyn Fn() -> bool,
    steps: &RefCell<Vec<String>>,
    saved: &RefCell<Vec<SavedDevice>>,
) -> Result<SavedDevice> {
    add_device_in(
        fixture.dir.path(),
        AddRequest {
            choice: Choice::Existing(fixture.key.clone()),
            install: false,
        },
        cancelled,
        &|step| steps.borrow_mut().push(step.text()),
        || Ok(Vec::new()),
        |device| {
            saved.borrow_mut().push(device);
            Ok(())
        },
    )
}

#[test]
fn a_key_with_no_vm_gets_one_from_an_80x32_terminal_once_then_only_headless_runs() {
    let Some(fixture) = fixture() else { return };
    let ssh = stand_in::install_script(&railway(CREATES));
    let (steps, saved) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
    let device = run(&fixture, &|| false, &steps, &saved).unwrap();
    assert_eq!(
        ssh.runs(),
        ["refused", "terminal 32 80", "reach", "probe"],
        "a bare probe, one terminal session, a bare probe, then the installer's probe"
    );
    assert_eq!(*steps.borrow(), ["Connecting…", "Creating…", "Connecting…"]);
    assert_eq!(device.id, fixture.id);
    assert!(
        identity::load(fixture.dir.path(), &fixture.id)
            .unwrap()
            .unwrap()
            .attached
    );

    // The VM exists now: setting up again never opens a terminal.
    steps.borrow_mut().clear();
    run(&fixture, &|| false, &steps, &saved).unwrap();
    assert_eq!(ssh.runs()[4..], ["reach", "probe"]);
    assert_eq!(*steps.borrow(), ["Connecting…"]);
}

#[test]
fn refusals_report_railways_words_without_its_links_and_never_loop_or_save() {
    let Some(fixture) = fixture() else { return };
    let cases: [(&str, &str, &[&str], &str); 4] = [
        // The daily limit comes back on the probe: same exit status as the
        // headless refusal, but a terminal session would not change it.
        (
            "exit 0",
            "limit on the probe",
            &["refused"],
            "Anonymous visitors are limited",
        ),
        // The same refusal from the terminal session itself.
        (
            "    echo 'Anonymous visitors are limited. Sign up: https://railway.com/signup?code=secret'\n    exit 13",
            "limit on the session",
            &["refused"],
            "Anonymous visitors are limited",
        ),
        // A session that ends without a VM is not retried.
        (
            "    exit 0",
            "no vm",
            &["refused", "refused"],
            "without creating",
        ),
        (
            "    echo 'down: https://railway.com/claim?code=secret'\n    exit 1",
            "failure",
            &["refused"],
            "down",
        ),
    ];
    for (terminal, name, runs, expected) in cases {
        let script = if name == "limit on the probe" {
            r#"echo refused >> '@LOG@'
echo 'Anonymous visitors are limited. Sign up: https://railway.com/signup?code=secret'
exit 13
"#
            .to_owned()
        } else {
            railway(terminal)
        };
        let ssh = stand_in::install_script(&script);
        let (steps, saved) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
        let error = run(&fixture, &|| false, &steps, &saved)
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{name}: {error}");
        assert!(!error.contains("secret"), "{name}: {error}");
        assert_eq!(ssh.runs(), runs, "{name}");
        assert!(saved.borrow().is_empty(), "{name}");
    }
}

#[test]
fn a_generated_key_survives_a_failed_creation_for_the_next_attempt() {
    let Some(fixture) = fixture() else { return };
    let _ssh = stand_in::install_script(&railway("    exit 1"));
    let home = tempfile::tempdir().unwrap();
    let key = generate_key_in(fixture.dir.path(), Some(home.path()), "mine", &|| false).unwrap();
    let request = || AddRequest {
        choice: Choice::Generated,
        install: false,
    };
    for _ in 0..2 {
        let error = add_device_in(
            fixture.dir.path(),
            request(),
            &|| false,
            &|_| {},
            || Ok(Vec::new()),
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Unreachable(_)), "{error:?}");
        let waiting = identity::pending(fixture.dir.path()).unwrap().unwrap();
        assert_eq!(waiting.key_path(), key, "the same key is reused");
    }
}

#[test]
fn cancelling_during_creation_stops_the_session_and_saves_nothing() {
    let Some(fixture) = fixture() else { return };
    let _ssh = stand_in::install_script(&railway("    exec sleep 30"));
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        flag.store(true, Ordering::Release);
    });
    let (steps, saved) = (RefCell::new(Vec::new()), RefCell::new(Vec::new()));
    let started = std::time::Instant::now();
    let error = run(&fixture, &|| stop.load(Ordering::Acquire), &steps, &saved).unwrap_err();
    canceller.join().unwrap();
    assert!(
        matches!(error, Error::Cloud(cloud::Error::Cancelled)),
        "{error:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(saved.borrow().is_empty());
}
