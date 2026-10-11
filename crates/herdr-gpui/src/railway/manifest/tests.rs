#![allow(clippy::unwrap_used)]
use super::*;

/// The shape seen live, with its links replaced.
fn line(expires: &str) -> String {
    format!(
        r#"{{"status":"trial_ready","state":"running","description":"Railway VM","usage":"ssh","preview_url":"https://p.example/x","human_claim_url":"https://claim.example/secret","build_expires_at":"{expires}"}}"#
    )
}

const NOW: u64 = 1_791_748_566; // 2026-10-11T19:56:06Z

#[test]
fn the_deadline_is_read_from_the_manifest_line() {
    let output = format!("noise\n{}\ntrailing", line("2026-10-11T20:56:06Z"));
    assert_eq!(deadline(&output, NOW), Some(NOW + 3600));
    // A numeric offset means the same instant.
    assert_eq!(
        deadline(&line("2026-10-11T21:56:06+01:00"), NOW),
        Some(NOW + 3600)
    );
}

#[test]
fn anything_else_is_ignored_not_guessed_at() {
    for output in [
        String::new(),
        "not json".into(),
        r#"{"status":"refused"}"#.into(),
        line("tomorrow"),
        line("1969-12-31T23:59:59Z"),
        // Further out than any documented build window.
        line("2027-01-01T00:00:00Z"),
        r#"{"build_expires_at":12345}"#.into(),
        format!("[{}]", line("2026-10-11T20:56:06Z")),
    ] {
        assert_eq!(deadline(&output, NOW), None, "{output}");
    }
}

#[test]
fn a_deadline_already_past_is_still_a_deadline() {
    // Setup must see an ended window, not mistake it for an unknown one.
    assert_eq!(
        deadline(&line("2026-10-11T19:00:00Z"), NOW),
        Some(NOW - 3366)
    );
}

#[test]
fn links_are_hidden_wherever_they_sit() {
    let hidden = hide_links(&line("2026-10-11T20:56:06Z"));
    assert!(!hidden.contains("claim.example") && !hidden.contains("secret"));
    assert!(!hidden.contains("p.example"));
    assert!(hidden.contains("[link hidden]") && hidden.contains("trial_ready"));
    assert_eq!(
        hide_links("see http://a.b/c now\nnext"),
        "see [link hidden] now\nnext"
    );
    assert_eq!(hide_links("nothing to hide"), "nothing to hide");
}
