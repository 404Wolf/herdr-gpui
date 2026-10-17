use super::*;

#[test]
fn activity_tracking_records_and_detects_idle() {
    let id = "test-vm-activity-id";
    record_activity(id);
    assert!(!is_idle(id, Duration::from_secs(60)));
    assert!(is_idle(id, Duration::from_millis(0)));

    forget_activity(id);
    assert!(!is_idle(id, Duration::from_secs(60)));
}

#[test]
fn a_vm_not_seen_yet_starts_its_timeout_instead_of_stopping() {
    let id = "test-vm-first-seen";
    forget_activity(id);
    assert!(!is_idle(id, Duration::from_secs(60)));
    assert!(is_idle(id, Duration::ZERO));
    forget_activity(id);
}

#[test]
fn only_this_accounts_devices_are_checked() {
    use crate::cloud::{CloudProvider, SavedDevice};
    let device = |account: &str, id: &str| SavedDevice {
        provider: CloudProvider::Azure,
        id: id.into(),
        label: id.into(),
        account: account.into(),
        machine: id.into(),
        session: "herdr".into(),
        enabled: true,
    };
    let devices = vec![
        device("sub-a", "shared-name"),
        device("sub-b", "shared-name"),
        device("sub-a", "sandbox.box"),
    ];
    let mine = account_devices(devices, "sub-a");
    let ids: Vec<&str> = mine.iter().map(|device| device.id.as_str()).collect();
    assert_eq!(ids, ["shared-name", "sandbox.box"]);
    assert!(mine.iter().all(|device| device.account == "sub-a"));
}

#[test]
fn a_device_being_destroyed_is_left_to_its_destroy() {
    use crate::cloud::{CloudProvider, SavedDevice};
    let device = SavedDevice {
        provider: CloudProvider::Azure,
        id: "being-destroyed".into(),
        label: "being-destroyed".into(),
        account: "sub-a".into(),
        machine: "being-destroyed".into(),
        session: "herdr".into(),
        enabled: true,
    };
    assert!(!super::super::destroy::destroying("being-destroyed"));
    assert_eq!(account_devices(vec![device.clone()], "sub-a").len(), 1);
    super::super::destroy::mark_for_test("being-destroyed", || {
        assert!(account_devices(vec![device.clone()], "sub-a").is_empty());
    });
    assert_eq!(account_devices(vec![device], "sub-a").len(), 1);
}
