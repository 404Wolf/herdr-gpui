use super::*;
use crate::control::server::hosts::security::{self as served, MAX_ALERTS, MAX_REPORTS, listing};
use crate::security::{Alert, Alerts, Error, Repo, Report, Reports, Severity};
use std::sync::Arc;

fn alert(number: u64) -> Alert {
    Alert {
        number,
        severity: Severity::High,
        package: "glib".into(),
        ecosystem: "rust".into(),
        vulnerable: "< 0.20.0".into(),
        patched: None,
        manifest: "Cargo.lock".into(),
        advisory: "GHSA-wrw7-89jp-8q8g".into(),
        summary: "Unsound iterator".into(),
        created_at: None,
    }
}

fn repo(name: &str, alerts: Result<Alerts, Error>) -> Repo {
    Repo {
        name: name.into(),
        host: "Local".into(),
        alerts: alerts.map_err(Arc::new),
        reports: Ok(Reports::Off),
    }
}

#[test]
fn the_listing_states_each_repository_and_links_each_alert() {
    let (repositories, cut) = listing(&[
        repo(
            "a/open",
            Ok(Alerts::Open {
                alerts: vec![alert(3)],
                total: 4,
            }),
        ),
        repo("a/off", Ok(Alerts::Off)),
        repo("a/failed", Err(Error::Output)),
    ]);
    assert!(!cut);
    let states: Vec<&str> = repositories
        .iter()
        .map(|repo| repo.state.as_str())
        .collect();
    assert_eq!(states, ["open", "off", "error"]);
    assert_eq!(repositories[0].total, 4);
    let first = &repositories[0].alerts[0];
    assert_eq!(first.severity, "high");
    assert_eq!(first.url, "https://github.com/a/open/security/dependabot/3");
    assert!(repositories[2].error.is_some());
}

#[test]
fn the_listing_stops_at_its_bound_and_says_so() {
    let many = |name| {
        repo(
            name,
            Ok(Alerts::Open {
                alerts: (0..100).map(alert).collect(),
                total: 100,
            }),
        )
    };
    let (repositories, cut) = listing(&[many("a/one"), many("a/two")]);
    assert!(cut);
    let carried: usize = repositories.iter().map(|repo| repo.alerts.len()).sum();
    assert_eq!(carried, MAX_ALERTS);
    // Each repository still counts every alert it has.
    assert!(repositories.iter().all(|repo| repo.total == 100));
}

#[gpui::test]
fn a_window_that_has_not_read_says_why(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    cx.update(|_, cx| {
        let Response::Host {
            reply:
                Reply::Security {
                    repositories,
                    note: Some(note),
                },
        } = served::alerts(view.read(cx))
        else {
            panic!("expected the security listing");
        };
        assert!(repositories.is_empty());
        assert!(note.contains("Sign in to GitHub"), "{note}");
    });
}

fn report(index: usize) -> Report {
    Report {
        advisory: format!("GHSA-{index:04}-bbbb-cccc"),
        summary: "Path traversal".into(),
        severity: None,
        reporter: Some("finder".into()),
        created_at: None,
    }
}

#[test]
fn reports_carry_their_state_and_link_and_stop_at_their_bound() {
    let mut hidden = repo("a/hidden", Ok(Alerts::Off));
    hidden.reports = Ok(Reports::Hidden);
    let mut many = repo("a/many", Ok(Alerts::Off));
    many.reports = Ok(Reports::Open((0..MAX_REPORTS + 5).map(report).collect()));
    let (repositories, cut) = listing(&[hidden, many]);
    assert!(cut);
    assert_eq!(repositories[0].reporting, "hidden");
    assert_eq!(repositories[1].reporting, "on");
    assert_eq!(repositories[1].reports.len(), MAX_REPORTS);
    let first = &repositories[1].reports[0];
    assert_eq!(first.severity, None);
    assert_eq!(
        first.url.as_deref(),
        Some("https://github.com/a/many/security/advisories/GHSA-0000-bbbb-cccc")
    );
}
