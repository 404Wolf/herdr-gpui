#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn the_window_constant_matches_railways_documented_build_time() {
    assert_eq!(WINDOW_SECONDS, 3600);
}

#[test]
fn the_saved_account_names_a_service_not_a_credential() {
    assert_eq!(ACCOUNT, "railway.new");
    assert!(!ACCOUNT.contains('@') && !ACCOUNT.contains(':') && !ACCOUNT.contains('/'));
}

#[test]
fn errors_name_what_the_user_can_do_next() {
    assert!(Error::WindowEnded.to_string().contains("Reconnect"));
    assert!(Error::WindowEnded.to_string().contains("claimed"));
    let missing = Error::KeyMissing("/home/u/.ssh/id".into()).to_string();
    assert!(missing.contains("/home/u/.ssh/id") && missing.contains("never replaced"));
    assert!(Error::UnusableKey("/k".into()).to_string().contains(".pub"));
}
