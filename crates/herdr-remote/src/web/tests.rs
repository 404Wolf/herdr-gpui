use super::*;
use crate::{
    auth::Limits,
    hub::Layout,
    tls::{Authority, Names},
};
use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::Request as HttpRequest,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tower::ServiceExt;

mod pairing;
mod routes;
mod uploads;

pub(super) const ORIGIN: &str = "https://192.168.1.20:7443";

/// A server state on a private directory with no daemon behind it.
pub(super) fn state(dir: &std::path::Path) -> Shared {
    let names = Names::new([IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20))], Some("studio"));
    let authority = Authority::load_or_create(dir).unwrap();
    Arc::new(State {
        apns: None,
        auth: Mutex::new(Auth::load(dir, Limits::default()).unwrap()),
        computer: "Studio".into(),
        connected: Mutex::new(HashMap::new()),
        fingerprint: authority.fingerprint().to_owned(),
        authority: Some(authority),
        hub: Hub::offline(Layout {
            connected: true,
            boot: "b".into(),
            workspaces: Vec::new(),
        }),
        names: RwLock::new(names),
        port: 7443,
        privacy: Privacy::Generic,
        push: None,
        uploads: Uploads::new(dir),
    })
}

/// Sends `request` through the router as a phone at 192.168.1.50 would.
pub(super) async fn send(
    state: &Shared,
    mut request: HttpRequest<Body>,
) -> (StatusCode, HeaderMap, String) {
    request
        .extensions_mut()
        .insert(ConnectInfo(Peer(SocketAddr::from((
            [192, 168, 1, 50],
            50_000,
        )))));
    let response = router(state.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

pub(super) fn get(path: &str) -> HttpRequest<Body> {
    HttpRequest::get(path).body(Body::empty()).unwrap()
}

pub(super) fn post(path: &str, body: serde_json::Value) -> HttpRequest<Body> {
    HttpRequest::post(path)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[test]
fn only_this_computers_origins_are_allowed() {
    let origins = vec![ORIGIN.to_owned()];
    assert!(allowed_origin(&origins, ORIGIN));
    assert!(!allowed_origin(&origins, "https://evil.example"));
    assert!(!allowed_origin(
        &origins,
        "https://192.168.1.20:7443.evil.example"
    ));
}

#[test]
fn a_bearer_token_is_read_from_its_header() {
    let mut headers = HeaderMap::new();
    assert_eq!(bearer(&headers), None);
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer abc"),
    );
    assert_eq!(bearer(&headers), Some("abc"));
    headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic abc"));
    assert_eq!(bearer(&headers), None);
    headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
    assert_eq!(bearer(&headers), None);
}

#[test]
fn branch_names_follow_gits_rules() {
    for good in ["feat/remote", "fix-1.2", "a_b"] {
        assert!(api::valid_branch(good), "{good}");
    }
    for bad in [
        "-x", "a..b", "a b", "a/", "x.lock", "a@{1}", "/a", "a//b", "a;rm",
    ] {
        assert!(!api::valid_branch(bad), "{bad}");
    }
}
