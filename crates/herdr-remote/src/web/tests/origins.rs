//! An installed app's home origin keeps working after the computer's
//! addresses change.

use super::*;

#[tokio::test]
async fn an_origin_served_before_still_passes_cors() {
    let dir = tempfile::tempdir().unwrap();
    let state = state(dir.path());
    let retired = "https://192.168.7.5:7443";
    let ping = |origin: &str| {
        HttpRequest::get("/api/ping")
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(send(&state, ping(retired)).await.0, StatusCode::FORBIDDEN);
    state
        .seen
        .lock()
        .unwrap()
        .remember(&[retired.to_owned()])
        .unwrap();
    // The current origins replace the old ones but do not forget them.
    let current = state.origins();
    state.seen.lock().unwrap().remember(&current).unwrap();
    assert_eq!(send(&state, ping(retired)).await.0, StatusCode::OK);
    assert_eq!(send(&state, ping(ORIGIN)).await.0, StatusCode::OK);
    assert_eq!(
        send(&state, ping("https://evil.example")).await.0,
        StatusCode::FORBIDDEN
    );
    // They survive a restart.
    let reloaded = Origins::load(dir.path());
    assert!(reloaded.contains(retired) && reloaded.contains(ORIGIN));
}

#[test]
fn only_the_newest_origins_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut origins = Origins::load(dir.path());
    for n in 0..(crate::web::origins::MAX_ORIGINS + 5) {
        origins
            .remember(&[format!("https://10.0.0.{n}:7443")])
            .unwrap();
    }
    assert!(origins.contains(&format!(
        "https://10.0.0.{}:7443",
        crate::web::origins::MAX_ORIGINS + 4
    )));
    assert!(!origins.contains("https://10.0.0.0:7443"));
    let reloaded = Origins::load(dir.path());
    assert!(!reloaded.contains("https://10.0.0.4:7443"));
    assert!(reloaded.contains("https://10.0.0.5:7443"));
}
