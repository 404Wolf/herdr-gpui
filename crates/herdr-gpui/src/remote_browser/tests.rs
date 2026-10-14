#![allow(clippy::unwrap_used)]
use super::*;

/// The shim run with a fake `herdr`, as an agent on a host runs it.
#[cfg(unix)]
mod shim;

/// The scripts that put the shim and the skill on a host.
#[cfg(unix)]
mod install;

/// Windows acting on requests in their snapshots. Opening a tab without
/// switching to it makes no native page, but only builds that show pages
/// open a tab at all.
#[cfg(any(target_os = "macos", windows))]
mod window;

pub(super) fn tokens(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn a_request_is_read_from_its_tokens() {
    let request = Request::from_tokens(&tokens(&[
        ("ci", "green"),
        ("herdr-gpui-do", "open"),
        ("herdr-gpui-id", "17-42"),
        ("herdr-gpui-pane", "w1:p2"),
        ("herdr-gpui-url-0", "http://localhost:3000/a"),
        ("herdr-gpui-url-1", "?b=c"),
    ]))
    .unwrap();
    assert_eq!(
        request,
        Request {
            id: "17-42".into(),
            pane: Some("w1:p2".into()),
            action: Action::Open {
                address: "http://localhost:3000/a?b=c".into(),
                focus: true,
            },
        }
    );
    let quiet = Request::from_tokens(&tokens(&[
        ("herdr-gpui-do", "open-background"),
        ("herdr-gpui-id", "1"),
        ("herdr-gpui-url-0", "example.com"),
    ]))
    .unwrap();
    assert_eq!(
        quiet.action,
        Action::Open {
            address: "example.com".into(),
            focus: false
        }
    );
    assert_eq!(quiet.pane, None);
    let reload = Request::from_tokens(&tokens(&[
        ("herdr-gpui-do", "reload"),
        ("herdr-gpui-id", "2"),
        ("herdr-gpui-pane", "p"),
    ]))
    .unwrap();
    assert_eq!(reload.action, Action::Reload);
}

#[test]
fn incomplete_or_unknown_requests_are_ignored() {
    let open = |extra: &[(&str, &str)]| {
        let mut pairs = vec![("herdr-gpui-do", "open"), ("herdr-gpui-id", "1")];
        pairs.extend_from_slice(extra);
        Request::from_tokens(&tokens(&pairs))
    };
    // No address, or one that skips its first part.
    assert_eq!(open(&[]), None);
    assert_eq!(open(&[("herdr-gpui-url-1", "x")]), None);
    // A gap ends the address at the last part before it.
    assert_eq!(
        open(&[("herdr-gpui-url-0", "a.test"), ("herdr-gpui-url-2", "/x")])
            .unwrap()
            .action,
        Action::Open {
            address: "a.test".into(),
            focus: true
        }
    );
    // Parts past the last number are never read.
    let url_parts: Vec<(String, String)> = (0..=URL_CHUNKS)
        .map(|index| (format!("{URL}{index}"), "a".to_owned()))
        .collect();
    let mut many = tokens(&[("herdr-gpui-do", "open"), ("herdr-gpui-id", "1")]);
    many.extend(url_parts);
    let Some(Request {
        action: Action::Open { address, .. },
        ..
    }) = Request::from_tokens(&many)
    else {
        panic!("expected an open request");
    };
    assert_eq!(address.len(), URL_CHUNKS);

    let without = |name: &str| {
        let pairs: Vec<_> = [
            ("herdr-gpui-do", "open"),
            ("herdr-gpui-id", "1"),
            ("herdr-gpui-url-0", "a.test"),
        ]
        .into_iter()
        .filter(|(key, _)| *key != name)
        .collect();
        Request::from_tokens(&tokens(&pairs))
    };
    assert_eq!(without("herdr-gpui-id"), None);
    assert_eq!(without("herdr-gpui-do"), None);
    for (key, value) in [
        ("herdr-gpui-do", "run"),
        ("herdr-gpui-id", "a b"),
        ("herdr-gpui-id", ""),
    ] {
        let pairs = [
            ("herdr-gpui-do", "open"),
            ("herdr-gpui-id", "1"),
            ("herdr-gpui-url-0", "a.test"),
        ]
        .map(|(name, old)| (name, if name == key { value } else { old }));
        assert_eq!(Request::from_tokens(&tokens(&pairs)), None, "{key}={value}");
    }
    // A pane that is not a plain identifier is dropped, not trusted.
    let odd = open(&[("herdr-gpui-url-0", "a.test"), ("herdr-gpui-pane", "p 1")]).unwrap();
    assert_eq!(odd.pane, None);
}

#[test]
fn request_tokens_are_told_from_other_plugins() {
    assert!(is_request_token("herdr-gpui-url-3"));
    assert!(!is_request_token("ci"));
    assert!(!is_request_token("herdr-status"));
}

#[test]
fn each_request_is_acted_on_once_and_the_memory_is_bounded() {
    let key = |id: usize, window: Option<u64>| SeenKey {
        scope: Scope::endpoint("host"),
        workspace: "w1".into(),
        id: id.to_string(),
        window,
    };
    let mut seen = Seen::default();
    assert!(seen.first(key(0, None)));
    assert!(!seen.first(key(0, None)));
    // A reload is each window's own.
    assert!(seen.first(key(0, Some(1))));
    assert!(seen.first(key(0, Some(2))));
    assert!(!seen.first(key(0, Some(2))));
    for id in 1..=MAX_SEEN {
        assert!(seen.first(key(id, None)));
    }
    assert_eq!(seen.0.len(), MAX_SEEN);
    // The oldest were forgotten to make room.
    assert!(seen.first(key(0, None)));
}
