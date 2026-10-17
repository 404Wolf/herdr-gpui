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
fn only_this_accounts_vms_are_checked() {
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
    let vms = account_vms(devices, "sub-a");
    assert_eq!(vms.len(), 1);
    assert_eq!(vms[0].account, "sub-a");
    assert_eq!(vms[0].id, "shared-name");
}
