#![allow(clippy::unwrap_used)]
use super::*;
use crate::railway::identity::Record;
use std::{fs, path::PathBuf, sync::atomic::AtomicBool};

/// A key `ssh-keygen` made at `dir/name`, saved as an attached identity.
fn saved_key(dir: &Path, name: &str) -> (PathBuf, String) {
    let key = dir.join(name);
    let made = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&key)
        .status();
    assert!(made.is_ok_and(|status| status.success()), "ssh-keygen");
    let id = keys::fingerprint(&key, &|| false).unwrap();
    let record = Record {
        attached: true,
        ..Record::unattached(&id, &key.display().to_string())
    };
    identity::save(dir, record).unwrap();
    (key, id)
}

fn connect(dir: &Path, id: &str) -> Result<Bridge> {
    connect_saved(dir, id, "default", &AtomicBool::new(false))
}

#[test]
fn a_device_without_a_usable_identity_is_reported_never_given_one() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        connect(dir.path(), "key-abcd"),
        Err(Error::NoIdentity)
    ));
    let (key, id) = saved_key(dir.path(), "id");
    let before = fs::read_dir(dir.path()).unwrap().count();
    fs::remove_file(&key).unwrap();
    assert!(matches!(
        connect(dir.path(), &id),
        Err(Error::KeyMissing(_))
    ));
    // Nothing was generated to take the missing key's place.
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), before - 1);
}

#[test]
fn a_replaced_key_file_never_reaches_another_vm() {
    let dir = tempfile::tempdir().unwrap();
    let (key, id) = saved_key(dir.path(), "id");
    let (other, _) = saved_key(dir.path(), "other");
    fs::copy(&other, &key).unwrap();
    fs::copy(identity::public_path(&other), identity::public_path(&key)).unwrap();
    let ssh = ssh::stand_in::install();
    assert!(matches!(
        connect(dir.path(), &id),
        Err(Error::KeyChanged(_))
    ));
    assert!(ssh.runs().is_empty(), "ssh must not run with another key");
}

/// A reconnect is headless every time, with a config that still exists when
/// `ssh` starts (the regression behind "Can't open user config file …: No such
/// file or directory"). Only setup ever runs a terminal session, which is what
/// can create a VM.
#[test]
fn a_reconnect_is_headless_and_hands_ssh_a_config_that_still_exists() {
    let dir = tempfile::tempdir().unwrap();
    let (_, id) = saved_key(dir.path(), "id");
    let ssh = ssh::stand_in::install_script(
        r#"mode=headless; config=
while [ $# -gt 0 ]; do
    case "$1" in -F) config=$2; shift 2;; -tt) mode=terminal; shift;; *) shift;; esac
done
[ -r "$config" ] && state=readable || state=missing
echo "$mode $state" >> '@LOG@'
exit 255
"#,
    );
    for _ in 0..3 {
        assert!(matches!(connect(dir.path(), &id), Err(Error::Bridge(_))));
    }
    assert_eq!(ssh.runs(), ["headless readable"; 3]);
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
