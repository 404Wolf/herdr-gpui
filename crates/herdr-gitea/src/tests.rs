use super::*;
use crate::testing::{Server, ok};
use secrecy::SecretString;
use std::{sync::Arc, time::Duration};

fn hosts() -> Vec<Host> {
    let config = HostConfig {
        host: "gitea.example.com".into(),
        aliases: vec!["gitea-ssh".into()],
        ..HostConfig::default()
    };
    let prefixed = HostConfig {
        host: "code.example.org".into(),
        url: Some("https://code.example.org/forge/".into()),
        ..HostConfig::default()
    };
    [config, prefixed]
        .iter()
        .filter_map(Host::from_config)
        .collect()
}

fn names(remote: &str) -> Option<(String, String, String)> {
    repo_in(&hosts(), remote).map(|repo| (repo.host.host, repo.owner, repo.name))
}

#[test]
fn remotes_of_every_shape_name_their_repository() {
    let expected = Some((
        "gitea.example.com".to_owned(),
        "instant-fill".to_owned(),
        "monopoly".to_owned(),
    ));
    for remote in [
        "ssh://gitea@gitea.example.com:2222/instant-fill/monopoly.git",
        "ssh://git@gitea.example.com/instant-fill/monopoly",
        "git@gitea.example.com:instant-fill/monopoly.git",
        "gitea.example.com:instant-fill/monopoly",
        "https://gitea.example.com/instant-fill/monopoly.git",
        "https://user:secret@gitea.example.com:8443/instant-fill/monopoly",
        "GITEA.EXAMPLE.COM:instant-fill/monopoly.git",
        "git@gitea-ssh:instant-fill/monopoly.git",
    ] {
        assert_eq!(names(remote), expected, "{remote}");
    }
}

#[test]
fn a_path_prefixed_instance_strips_its_prefix_from_http_remotes_only() {
    let expected = Some((
        "code.example.org".to_owned(),
        "team".to_owned(),
        "app".to_owned(),
    ));
    assert_eq!(
        names("https://code.example.org/forge/team/app.git"),
        expected
    );
    assert_eq!(names("git@code.example.org:team/app.git"), expected);
}

#[test]
fn other_hosts_and_hostile_remotes_are_not_gitea() {
    for remote in [
        "git@github.com:owner/repo.git",
        "https://gitea.example.com.evil.com/o/r",
        "https://evil.com/gitea.example.com/o/r",
        "https://a@b@gitea.example.com/o/r",
        "ssh://gitea.example.com:22x/o/r",
        "ftp://gitea.example.com/o/r",
        "https://gitea.example.com/../r",
        "https://gitea.example.com/o",
        "https://gitea.example.com/o/r%2F..",
        "/srv/git/gitea.example.com/o/r",
        "",
    ] {
        assert_eq!(names(remote), None, "{remote}");
    }
}

#[test]
fn config_checks_hosts_urls_and_commands() {
    let host = |host: &str, url: Option<&str>| HostConfig {
        host: host.into(),
        url: url.map(Into::into),
        ..HostConfig::default()
    };
    let config = |hosts| Config { hosts };
    assert!(
        config(vec![host("gitea.example.com", None)])
            .check()
            .is_ok()
    );
    assert!(
        config(vec![host(
            "gitea.example.com",
            Some("http://10.0.0.2:3000")
        )])
        .check()
        .is_ok()
    );
    assert_eq!(
        config(vec![host("https://gitea.example.com", None)]).check(),
        Err(config::ConfigError::Host(
            "https://gitea.example.com".into()
        ))
    );
    assert_eq!(
        config(vec![host("a.example", Some("ftp://a.example"))]).check(),
        Err(config::ConfigError::Url("ftp://a.example".into()))
    );
    assert_eq!(
        config(vec![host("a.example", Some("https://user:pw@a.example"))]).check(),
        Err(config::ConfigError::Url("https://user:pw@a.example".into()))
    );
    assert_eq!(
        config(vec![host("a.example", None), host("A.example", None)]).check(),
        Err(config::ConfigError::Duplicate("a.example".into()))
    );
    let relative_ca = HostConfig {
        ca_file: Some("ca.pem".into()),
        ..host("a.example", None)
    };
    assert_eq!(
        config(vec![relative_ca]).check(),
        Err(config::ConfigError::CaFile("ca.pem".into()))
    );
    let empty_command = HostConfig {
        token_command: vec![String::new()],
        ..host("a.example", None)
    };
    assert!(config(vec![empty_command]).check().is_err());
}

#[test]
fn config_parses_from_toml() {
    let config: Config = serde_json::from_value(serde_json::json!({
        "hosts": [{"host": "gitea.example.com", "token_command": ["cat", "/run/secret"]}]
    }))
    .unwrap_or_default();
    assert_eq!(config.hosts.len(), 1);
    assert_eq!(config.hosts[0].token_command, ["cat", "/run/secret"]);
    let host = Host::from_config(&config.hosts[0]);
    assert_eq!(
        host.map(|host| (host.base, host.secure)),
        Some(("https://gitea.example.com".to_owned(), true))
    );
}

#[test]
fn pull_urls_name_their_repository_and_number() {
    configure(&Config {
        hosts: vec![HostConfig {
            host: "pulls.example.com".into(),
            ..HostConfig::default()
        }],
    });
    let parsed = pull_url("https://pulls.example.com/o/r/pulls/12")
        .map(|(repo, number)| (repo.owner, repo.name, number));
    assert_eq!(parsed, Some(("o".into(), "r".into(), 12)));
    for url in [
        "https://pulls.example.com/o/r/pulls/012",
        "https://pulls.example.com/o/r/issues/12",
        "https://pulls.example.com/o/r/pulls/12/files",
        "https://pulls.example.com.evil/o/r/pulls/12",
        "https://github.com/o/r/pull/12",
    ] {
        assert!(pull_url(url).is_none(), "{url}");
    }
}

#[test]
fn token_output_is_trimmed_and_validated() {
    let parsed = |bytes: &[u8]| {
        token::parse(bytes, "h")
            .map(|token| token.map(|token| secrecy::ExposeSecret::expose_secret(&token).to_owned()))
    };
    assert_eq!(
        parsed(b"abc123\n").ok().flatten().as_deref(),
        Some("abc123")
    );
    assert!(matches!(parsed(b"  \n"), Ok(None)));
    assert!(parsed(b"two words").is_err());
    assert!(parsed(&[0xff, 0xfe]).is_err());
}

#[cfg(unix)]
#[test]
fn a_token_command_is_run_and_its_answer_reused() {
    let directory = tempfile::tempdir().ok();
    let Some(directory) = directory else { return };
    let counter = directory.path().join("runs");
    let script = format!("echo x >> {}; printf 'tok-1\\n'", counter.display());
    let host = Host {
        host: "token.example.com".into(),
        names: vec!["token.example.com".into()],
        base: "https://token.example.com".into(),
        secure: true,
        token_command: vec!["sh".into(), "-c".into(), script],
        ca_file: None,
    };
    let first = token(&host).ok().flatten();
    let second = token(&host).ok().flatten();
    assert_eq!(
        first
            .as_deref()
            .map(|token| secrecy::ExposeSecret::expose_secret(token).to_owned())
            .as_deref(),
        Some("tok-1")
    );
    assert!(second.is_some());
    let runs = std::fs::read_to_string(&counter).unwrap_or_default();
    assert_eq!(runs.lines().count(), 1);
    // Over plain HTTP the token is never read, let alone sent.
    let insecure = Host {
        secure: false,
        host: "plain.example.com".into(),
        ..host
    };
    assert!(matches!(token(&insecure), Ok(None)));
}

#[test]
fn refusals_say_what_the_user_can_do() {
    let url = "https://h/api/v1/repos/o/r";
    assert!(matches!(
        http::status_error(401, url, b""),
        Error::Authentication(_)
    ));
    assert!(matches!(
        http::status_error(404, url, b""),
        Error::NotFound(_)
    ));
    assert!(matches!(
        http::status_error(405, url, br#"{"message":"Please try again later"}"#),
        Error::Refused(message) if message == "Please try again later"
    ));
    assert!(matches!(
        http::status_error(500, url, b""),
        Error::Status(500)
    ));
}

#[test]
fn listings_follow_pages_until_the_total_is_read() {
    let page = |numbers: std::ops::Range<u64>| {
        serde_json::Value::Array(
            numbers
                .map(|number| serde_json::json!({"number": number}))
                .collect(),
        )
    };
    let total = vec![("X-Total-Count".to_owned(), "60".to_owned())];
    let routes = vec![
        (
            "GET",
            "/api/v1/repos/o/r/pulls?state=open&limit=50&page=1".to_owned(),
            200,
            total.clone(),
            page(1..51).to_string(),
        ),
        (
            "GET",
            "/api/v1/repos/o/r/pulls?state=open&limit=50&page=2".to_owned(),
            200,
            total,
            page(51..61).to_string(),
        ),
    ];
    let Ok(server) = Server::start(routes) else {
        return;
    };
    let host = server.host("h");
    let token = Some(Arc::new(SecretString::from("secret-token")));
    let client = Client::with_token(&host, token, Duration::from_secs(5));
    let (items, complete): (Vec<PullRequest>, bool) = client
        .get_pages(&host.api("/repos/o/r/pulls?state=open"), MAX_PAGES, &|| {
            false
        })
        .unwrap_or_default();
    assert!(complete);
    assert_eq!(items.len(), 60);
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].2.as_deref(), Some("token secret-token"));
}

#[test]
fn a_missing_object_is_none_and_cancellation_stops_before_asking() {
    let Ok(server) = Server::start(vec![ok("GET", "/api/v1/repos/o/r", serde_json::json!({}))])
    else {
        return;
    };
    let host = server.host("h");
    let client = Client::with_token(&host, None, Duration::from_secs(5));
    let optional: Result<Option<Repository>> =
        client.get_optional(&host.api("/repos/o/gone"), &|| false);
    assert!(matches!(optional, Ok(None)));
    // Needed and read anonymously, a 404 may be a private repository, so it
    // asks for a token.
    let required: Result<Repository> = client.get(&host.api("/repos/o/gone"), &|| false);
    assert!(matches!(required, Err(Error::Authentication(_))));
    let cancelled: Result<Repository> = client.get(&host.api("/repos/o/r"), &|| true);
    assert!(matches!(cancelled, Err(Error::Cancelled)));
    // Only the two lookups of the missing repository reached the server.
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn a_bundle_file_yields_its_certificates_and_a_missing_one_none() {
    let directory = tempfile::tempdir().ok();
    let Some(directory) = directory else { return };
    // One self-signed certificate, as a private CA's would be.
    let pem = "-----BEGIN CERTIFICATE-----\nMIIBfzCCASWgAwIBAgIUQvb3ZP3E4N4zT1lJxQ7ZBc3p3HQwCgYIKoZIzj0EAwIw\nFTETMBEGA1UEAwwKdGVzdC1jYS0xMB4XDTI2MDEwMTAwMDAwMFoXDTM2MDEwMTAw\nMDAwMFowFTETMBEGA1UEAwwKdGVzdC1jYS0xMFkwEwYHKoZIzj0CAQYIKoZIzj0D\nAQcDQgAEjvW2Qy2ZJ2rB4V9Yw0v5bnWw9Sxp1E4Y0Hq3M6oX3d8m1QnqV7k2k1w2\nH0sW9t9b3m3z2r8J7xg6H2fX5o9QvqNTMFEwHQYDVR0OBBYEFGt0ZXN0LWNhLTEt\na2V5LWlkMB8GA1UdIwQYMBaAFGt0ZXN0LWNhLTEta2V5LWlkMA8GA1UdEwEB/wQF\nMAMBAf8wCgYIKoZIzj0EAwIDSAAwRQIgYXJiaXRyYXJ5LXNpZ25hdHVyZS1ieXRl\ncwIhAJl0ZXN0LW9ubHktbm90LXZlcmlmaWVkLWJ5LWFueXRoaW5nAA==\n-----END CERTIFICATE-----\n";
    let path = directory.path().join("ca.pem");
    if std::fs::write(&path, pem).is_err() {
        return;
    }
    assert_eq!(tls::read(&path).len(), 1);
    assert!(tls::read(&directory.path().join("missing.pem")).is_empty());
    assert!(tls::read(directory.path()).is_empty());
}

#[test]
fn nulls_gitea_sends_for_empty_fields_read_as_defaults() {
    // As Gitea 1.26 answers a commit with no statuses reported.
    let status: std::result::Result<CombinedStatus, _> = serde_json::from_value(
        serde_json::json!({"state": "pending", "total_count": 0, "statuses": null}),
    );
    assert!(status.is_ok_and(|status| status.statuses.is_empty()));
    let issue: std::result::Result<Issue, _> = serde_json::from_value(
        serde_json::json!({"number": 1, "title": null, "body": null, "labels": null}),
    );
    assert!(issue.is_ok_and(|issue| issue.title.is_empty() && issue.labels.is_empty()));
}
