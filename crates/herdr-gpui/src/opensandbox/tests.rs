#![allow(clippy::unwrap_used)]
use super::*;
use crate::config::OpenSandboxConfig;

mod live;
mod relay;

fn settings(url: &str) -> Settings {
    Settings::resolve(
        &OpenSandboxConfig {
            api_url: Some(url.into()),
            ..OpenSandboxConfig::default()
        },
        |_| None,
    )
    .unwrap()
    .unwrap()
}

#[test]
fn the_server_must_be_https_or_on_this_computer() {
    assert_eq!(
        settings("http://127.0.0.1:8080/").base,
        "http://127.0.0.1:8080"
    );
    assert_eq!(
        settings("https://sandbox.example.com").endpoint_scheme(),
        "https"
    );
    assert_eq!(settings("http://localhost:8080").endpoint_scheme(), "http");
    for bad in [
        "http://10.0.0.5:8080",
        "https://user:pass@sandbox.example.com",
        "https://sandbox.example.com/?x=1",
    ] {
        let config = OpenSandboxConfig {
            api_url: Some(bad.into()),
            ..OpenSandboxConfig::default()
        };
        assert!(Settings::resolve(&config, |_| None).is_err(), "{bad}");
    }
}

#[test]
fn sandbox_sizes_are_kubernetes_quantities() {
    use super::settings::quantity;
    for good in ["2", "0.5", "500m", "4Gi", "512Mi", "1G"] {
        assert!(quantity(good), "{good}");
    }
    for bad in ["", "Gi", "-1", "4 Gi", "4gb", "0", "1e9"] {
        assert!(!quantity(bad), "{bad}");
    }
    let defaults = settings("http://127.0.0.1:8080");
    assert_eq!(
        (
            defaults.image.as_str(),
            defaults.cpu.as_str(),
            defaults.memory.as_str()
        ),
        ("buildpack-deps:bookworm", "2", "4Gi")
    );
}

#[test]
fn sandbox_ids_stay_plain() {
    use super::settings::sandbox_id;
    assert!(sandbox_id("2e7db059-99af-4060-bf83-d0d00b355e24"));
    for bad in ["", "-x", "a/b", "a b", "a?b"] {
        assert!(!sandbox_id(bad), "{bad}");
    }
}

#[test]
fn error_replies_keep_one_short_line() {
    assert_eq!(
        http::message(br#"{"detail":{"code":"X","message":"no\nsuch sandbox"}}"#).as_deref(),
        Some("no such sandbox")
    );
    assert_eq!(
        http::message(br#"{"message":"quota"}"#).as_deref(),
        Some("quota")
    );
    assert!(matches!(
        http::failure("t", 401, b""),
        Error::Authentication
    ));
}
