use super::*;
use crate::orchestrator::gitlab::{
    IssueNode, MAX_OPEN, Page, environment_token, full, issues_url, next_page, parse_cli,
    retry_after, status_error,
};
use crate::orchestrator::service::retry_wait;
use secrecy::ExposeSecret;
use std::{
    collections::HashMap,
    ffi::OsString,
    time::{Duration, SystemTime},
};

fn gitlab() -> SourceKey {
    SourceKey {
        provider: Provider::Gitlab,
        host: "gitlab.com".into(),
        repository: "group/sub/app".into(),
    }
}

fn node(iid: u64) -> IssueNode {
    serde_json::from_value(serde_json::json!({
        "id": 9000 + iid,
        "iid": iid,
        "title": format!("Issue {iid}"),
        "description": "Body",
        "state": "opened",
        "web_url": format!("https://gitlab.com/group/sub/app/-/issues/{iid}"),
        "created_at": "2026-10-08T23:23:37.000Z",
        "updated_at": "2026-10-09T01:00:00.500Z",
        "author": {"id": 1, "username": "penso", "name": "Penso"},
        "labels": ["bug"],
        "user_notes_count": 2,
    }))
    .unwrap()
}

fn page(from: u64, count: u64, next: Option<u32>) -> Page {
    Page {
        issues: (from..from + count).map(node).collect(),
        next,
    }
}

#[test]
fn an_issue_reads_as_agent_launcher_stores_it() {
    let item = node(42).into_item(&gitlab());
    assert_eq!(item.key.native_id, "42");
    assert_eq!(
        item.key.canonical(),
        "gitlab:gitlab.com:group/sub/app:42",
        "keyed by the project's iid, not GitLab's global id"
    );
    assert_eq!(item.key.canonical().parse::<ItemKey>().unwrap(), item.key);
    assert_eq!(item.identifier, "#42");
    assert_eq!(item.state, "open");
    assert_eq!(item.author.as_deref(), Some("penso"));
    assert_eq!(item.labels, ["bug"]);
    assert_eq!(item.description.as_deref(), Some("Body"));
    assert_eq!(item.updated_at, Some(at("2026-10-09T01:00:00.5+00:00")));
    assert_eq!(
        item.activity.and_then(|activity| activity.comments),
        Some(2)
    );
    assert!(item.pull_request.is_none());
}

#[test]
fn an_issue_without_optional_fields_still_decodes() {
    let node: IssueNode = serde_json::from_value(serde_json::json!({
        "iid": 1, "title": "Bare", "description": null, "state": "closed",
        "web_url": "https://gitlab.com/g/a/-/issues/1",
        "created_at": "2026-10-08T23:23:37Z", "updated_at": "2026-10-08T23:23:37Z",
        "author": null,
    }))
    .unwrap();
    let item = node.into_item(&gitlab());
    assert_eq!(item.state, "closed");
    assert!(item.description.is_none() && item.author.is_none() && item.labels.is_empty());
}

#[test]
fn the_project_path_is_one_escaped_segment() {
    let url = issues_url(&gitlab(), 3).unwrap();
    assert_eq!(url.host_str(), Some("gitlab.com"));
    assert_eq!(url.path(), "/api/v4/projects/group%2Fsub%2Fapp/issues");
    let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(query["state"], "opened");
    assert_eq!(query["page"], "3");
    assert_eq!(query["per_page"], "100");

    let managed = SourceKey {
        host: "gitlab.acme.com:8443".into(),
        ..gitlab()
    };
    let url = issues_url(&managed, 1).unwrap();
    assert_eq!(url.scheme(), "https");
    assert_eq!(url.port(), Some(8443));
}

#[test]
fn a_full_listing_reads_every_page_and_keeps_agent_launchers_markers() {
    let previous = Checkpoint {
        pr_details: HashMap::from([("pr/1".into(), "marker".into())]),
        pr_cursor: Some(7),
        ..Checkpoint::default()
    };
    let mut asked = Vec::new();
    let listing = full(&gitlab(), previous, &mut |number| {
        asked.push(number);
        Ok(match number {
            1 => page(1, 100, Some(2)),
            _ => page(101, 5, None),
        })
    })
    .unwrap();
    assert_eq!(asked, [1, 2]);
    assert!(listing.complete);
    assert_eq!(listing.items.len(), 105);
    assert!(listing.checkpoint.last_full_at.is_some());
    assert_eq!(listing.checkpoint.pr_details["pr/1"], "marker");
    assert_eq!(listing.checkpoint.pr_cursor, Some(7));
}

#[test]
fn a_listing_past_the_cap_is_shown_but_not_complete() {
    let mut pages = 0;
    let listing = full(&gitlab(), Checkpoint::default(), &mut |number| {
        pages += 1;
        Ok(page(u64::from(number) * 100, 100, Some(number + 1)))
    })
    .unwrap();
    assert!(!listing.complete);
    assert_eq!(listing.items.len(), MAX_OPEN);
    assert_eq!(pages, MAX_OPEN / 100);
}

#[test]
fn a_next_page_that_does_not_advance_ends_the_listing() {
    let mut pages = 0;
    let listing = full(&gitlab(), Checkpoint::default(), &mut |_| {
        pages += 1;
        Ok(page(1, 3, Some(1)))
    })
    .unwrap();
    assert_eq!(pages, 1);
    assert!(listing.complete);
}

#[test]
fn a_failed_page_fails_the_listing() {
    let result = full(
        &gitlab(),
        Checkpoint::default(),
        &mut |number| match number {
            1 => Ok(page(1, 100, Some(2))),
            _ => Err(Error::GitlabRateLimit),
        },
    );
    assert!(matches!(result, Err(Error::GitlabRateLimit)));
}

#[test]
fn refusals_say_whether_a_token_is_needed_or_wrong() {
    for status in [401, 403, 404] {
        assert!(matches!(
            status_error(status, "g/app", false),
            Error::GitlabSignIn { repository } if repository == "g/app"
        ));
    }
    assert!(matches!(
        status_error(401, "g/app", true),
        Error::GitlabAuthentication
    ));
    assert!(matches!(
        status_error(403, "g/app", true),
        Error::GitlabForbidden { .. }
    ));
    assert!(matches!(
        status_error(404, "g/app", true),
        Error::GitlabNotFound { .. }
    ));
    assert!(matches!(
        status_error(429, "g/app", true),
        Error::GitlabRateLimit
    ));
    assert!(matches!(
        status_error(502, "g/app", true),
        Error::GitlabStatus(502)
    ));
    assert!(
        status_error(404, "g/app", false)
            .to_string()
            .contains("GITLAB_TOKEN")
    );
}

#[test]
fn the_next_page_header_is_read_and_empty_means_last() {
    let mut headers = ureq::http::HeaderMap::new();
    assert_eq!(next_page(&headers), None);
    headers.insert("x-next-page", "".parse().unwrap());
    assert_eq!(next_page(&headers), None);
    headers.insert("x-next-page", "4".parse().unwrap());
    assert_eq!(next_page(&headers), Some(4));
}

#[test]
fn a_rate_limit_waits_as_long_as_gitlab_asked_within_bounds() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let mut headers = ureq::http::HeaderMap::new();
    assert_eq!(retry_after(500, &headers, now), None);
    assert_eq!(
        retry_after(429, &headers, now),
        Some(Duration::from_secs(3600))
    );
    headers.insert("ratelimit-reset", "1000900".parse().unwrap());
    assert_eq!(
        retry_after(429, &headers, now),
        Some(Duration::from_secs(900))
    );
    headers.insert("retry-after", "10".parse().unwrap());
    assert_eq!(
        retry_after(429, &headers, now),
        Some(Duration::from_secs(300)),
        "never sooner than five minutes"
    );
}

#[test]
fn gitlab_failures_wait_for_the_user_or_the_rate_limit() {
    let half_hour = Duration::from_secs(1800);
    let hour = Duration::from_secs(3600);
    let sign_in = Error::GitlabSignIn {
        repository: "g/app".into(),
    };
    assert_eq!(retry_wait(&sign_in, None), half_hour);
    assert_eq!(retry_wait(&Error::GitlabAuthentication, None), half_hour);
    assert_eq!(retry_wait(&Error::GitlabRateLimit, Some(hour)), hour);
    assert_eq!(
        retry_wait(&Error::GitlabRateLimit, None),
        Duration::from_secs(300)
    );
}

fn environment(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
    let pairs: HashMap<String, OsString> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), OsString::from(value)))
        .collect();
    move |name| pairs.get(name).cloned()
}

#[test]
fn an_environment_token_reaches_only_the_host_it_is_meant_for() {
    let token = |host, pairs: &[(&str, &str)]| {
        environment_token(host, environment(pairs)).map(|token| token.expose_secret().to_owned())
    };
    let plain = [("GITLAB_TOKEN", "glpat-abc")];
    assert_eq!(token("gitlab.com", &plain).as_deref(), Some("glpat-abc"));
    assert_eq!(token("gitlab.acme.com", &plain), None);

    let managed = [
        ("GITLAB_HOST", "https://gitlab.acme.com/"),
        ("GITLAB_ACCESS_TOKEN", "glpat-acme"),
    ];
    assert_eq!(
        token("gitlab.acme.com", &managed).as_deref(),
        Some("glpat-acme")
    );
    assert_eq!(token("gitlab.com", &managed), None);

    let bare = [
        ("GITLAB_HOST", "gitlab.acme.com:8443"),
        ("GITLAB_TOKEN", "t"),
    ];
    assert_eq!(token("gitlab.acme.com:8443", &bare).as_deref(), Some("t"));

    let first = [("GITLAB_TOKEN", "one"), ("GITLAB_ACCESS_TOKEN", "two")];
    assert_eq!(token("gitlab.com", &first).as_deref(), Some("one"));
    let broken = [
        ("GITLAB_TOKEN", "has space"),
        ("GITLAB_ACCESS_TOKEN", "two"),
    ];
    assert_eq!(token("gitlab.com", &broken).as_deref(), Some("two"));
}

#[test]
fn the_cli_answer_is_one_token_line_or_none() {
    let read = |output: &[u8]| parse_cli(output).map(|token| token.expose_secret().to_owned());
    assert_eq!(read(b"glpat-abc\n").as_deref(), Some("glpat-abc"));
    assert_eq!(read(b"\n"), None);
    assert_eq!(read(b""), None);
    assert_eq!(read(b"one\ntwo\n"), None);
    assert_eq!(read(b"not a token\n"), None);
    assert_eq!(read(&[0xff, 0xfe]), None);
}
