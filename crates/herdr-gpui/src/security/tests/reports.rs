use super::*;
use crate::security::reports::{hidden, parse, parse_enabled, report_url};

fn advisory(id: &str, state: &str, severity: Value, created: &str) -> Value {
    json!({
        "ghsa_id": id,
        "summary": format!("Report {id}"),
        "description": "Step 1: send this payload…",
        "state": state,
        "severity": severity,
        "author": {"login": "finder"},
        "created_at": created,
        "cvss": {"score": 9.8},
    })
}

#[test]
fn reads_reports_in_triage_rated_first_then_newest() {
    let reports = parse(&json!([
        advisory(
            "GHSA-aaaa-aaaa-aaaa",
            "triage",
            Value::Null,
            "2026-10-05T00:00:00Z"
        ),
        advisory(
            "GHSA-bbbb-bbbb-bbbb",
            "triage",
            json!("medium"),
            "2026-10-01T00:00:00Z"
        ),
        advisory(
            "GHSA-cccc-cccc-cccc",
            "triage",
            json!("critical"),
            "2026-10-02T00:00:00Z"
        ),
        advisory(
            "GHSA-dddd-dddd-dddd",
            "draft",
            json!("critical"),
            "2026-10-03T00:00:00Z"
        ),
    ]))
    .unwrap();
    let Reports::Open(reports) = reports else {
        panic!("expected open reports");
    };
    let order: Vec<&str> = reports
        .iter()
        .map(|report| report.advisory.as_str())
        .collect();
    // A maintainer's own draft is not a report.
    assert_eq!(
        order,
        [
            "GHSA-cccc-cccc-cccc",
            "GHSA-bbbb-bbbb-bbbb",
            "GHSA-aaaa-aaaa-aaaa"
        ]
    );
    assert_eq!(reports[1].severity, Some(Severity::Moderate));
    assert_eq!(reports[2].severity, None);
    assert_eq!(reports[0].reporter.as_deref(), Some("finder"));
}

#[test]
fn a_report_keeps_no_description() {
    let Reports::Open(reports) = parse(&json!([advisory(
        "GHSA-aaaa-aaaa-aaaa",
        "triage",
        json!("high"),
        "2026-10-05T00:00:00Z"
    )]))
    .unwrap() else {
        panic!("expected open reports");
    };
    let held = format!("{reports:?}");
    assert!(!held.contains("payload"), "{held}");
}

#[test]
fn reporting_switch_and_malformed_answers() {
    assert!(parse_enabled(&json!({"enabled": true})).unwrap());
    assert!(!parse_enabled(&json!({"enabled": false})).unwrap());
    assert!(matches!(parse_enabled(&json!({})), Err(Error::Json(_))));
    assert!(matches!(
        parse(&json!({"message": "x"})),
        Err(Error::Json(_))
    ));
}

#[test]
fn a_refusal_hides_reports_but_a_spent_quota_is_an_error() {
    assert!(hidden(&crate::Error::GitHubForbidden));
    assert!(hidden(&crate::Error::GitHubStatus(404)));
    assert!(!hidden(&crate::Error::GitHubStatus(500)));
    assert!(!hidden(&crate::Error::GitHubRateLimit { reset: None }));
}

#[test]
fn report_links_need_a_ghsa_identifier() {
    let good = report("GHSA-aaaa-bbbb-cccc", None, "2026-10-05T00:00:00Z");
    assert_eq!(
        report_url("penso/herdr-gpui", &good).as_deref(),
        Some("https://github.com/penso/herdr-gpui/security/advisories/GHSA-aaaa-bbbb-cccc")
    );
    let bad = report("../../x", None, "2026-10-05T00:00:00Z");
    assert_eq!(report_url("penso/herdr-gpui", &bad), None);
}

#[test]
fn every_report_counts_on_the_bar_and_reporting_makes_it_available() {
    let mut security = Security::default();
    security.install(
        vec![reported(
            "a/one",
            vec![
                report("GHSA-aaaa-aaaa-aaaa", None, "2026-10-05T00:00:00Z"),
                report(
                    "GHSA-bbbb-bbbb-bbbb",
                    Some(Severity::Low),
                    "2026-10-05T00:00:00Z",
                ),
            ],
        )],
        Instant::now(),
        SystemTime::now(),
    );
    assert!(security.available());
    assert_eq!(security.urgent(), 2);
    // Reporting on with an empty queue still lists the section.
    let mut quiet = Security::default();
    quiet.install(
        vec![reported("a/two", Vec::new())],
        Instant::now(),
        SystemTime::now(),
    );
    assert!(quiet.available());
    assert_eq!(quiet.urgent(), 0);
}
