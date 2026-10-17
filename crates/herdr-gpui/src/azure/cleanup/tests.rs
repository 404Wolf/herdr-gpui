use super::*;

const MINE: &str = "11111111-1111-1111-1111-111111111111";
const OTHER: &str = "22222222-2222-2222-2222-222222222222";

fn settings() -> Settings {
    let config = crate::config::AzureConfig {
        subscription_id: Some(MINE.into()),
        ..Default::default()
    };
    Settings::resolve(&config, |_| None)
        .expect("settings resolve")
        .expect("settings configured")
}

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
        device(MINE, "shared-name"),
        device(OTHER, "shared-name"),
        device(&format!("{OTHER}/herdr-devices"), "elsewhere"),
        device(&format!("{MINE}/other-rg/boxes/eastus"), "sandbox.box"),
    ];
    let mine = account_devices(devices, &settings());
    let ids: Vec<&str> = mine.iter().map(|device| device.id.as_str()).collect();
    assert_eq!(ids, ["shared-name", "sandbox.box"]);
}

#[test]
fn a_device_being_destroyed_is_left_to_its_destroy() {
    use crate::cloud::{CloudProvider, SavedDevice};
    let device = SavedDevice {
        provider: CloudProvider::Azure,
        id: "being-destroyed".into(),
        label: "being-destroyed".into(),
        account: MINE.into(),
        machine: "being-destroyed".into(),
        session: "herdr".into(),
        enabled: true,
    };
    assert!(!super::super::destroy::destroying("being-destroyed"));
    assert_eq!(account_devices(vec![device.clone()], &settings()).len(), 1);
    super::super::destroy::mark_for_test("being-destroyed", || {
        assert!(account_devices(vec![device.clone()], &settings()).is_empty());
    });
    assert_eq!(account_devices(vec![device], &settings()).len(), 1);
}
