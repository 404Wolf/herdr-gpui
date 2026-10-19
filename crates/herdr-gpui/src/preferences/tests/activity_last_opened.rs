//! When each activity bar section was last open is remembered with the rest
//! of the chrome, so a count stays muted across restarts until it changes.
use super::*;
use crate::sidebar::ActivityPanel;
use chrono::DateTime;

#[core::prelude::v1::test]
fn last_opened_times_round_trip_and_damaged_ones_are_forgotten() {
    let directory = TestDirectory::new();
    let path = directory.0.join("preferences.json");
    let at = |text: &str| DateTime::parse_from_rfc3339(text).unwrap().to_utc();
    let chrome = Chrome {
        activity_last_opened: [
            (ActivityPanel::PullRequests, at("2026-10-19T08:30:00Z")),
            (ActivityPanel::Security, at("2026-10-18T22:00:00.250Z")),
        ]
        .into(),
        ..Chrome::default()
    };
    write_chrome(&path, chrome.clone()).unwrap();
    assert_eq!(read_chrome(&path).unwrap(), chrome);

    // Earlier builds wrote none; an unknown section or unreadable time is
    // dropped alone, keeping the rest of the file.
    fs::write(
        &path,
        r#"{"sidebar_width_px": 250, "activity_last_opened_at": {
            "issues": "2026-10-19T10:00:00+02:00",
            "dashboard": "2026-10-19T10:00:00Z",
            "overview": "yesterday",
            "security": 3
        }}"#,
    )
    .unwrap();
    let read = read_chrome(&path).unwrap();
    assert_eq!(
        read.activity_last_opened,
        [(ActivityPanel::Issues, at("2026-10-19T08:00:00Z"))].into()
    );
    assert_eq!(read.sidebar_width, Some(250.));
    for stored in ["", r#", "activity_last_opened_at": []"#] {
        fs::write(&path, format!(r#"{{"sidebar_width_px": 250{stored}}}"#)).unwrap();
        assert!(read_chrome(&path).unwrap().activity_last_opened.is_empty());
    }
}
