use super::*;
use crate::window::status_bar::{
    clean_cpu_brand, extract_ioreg_string, format_mac_model, report_issue_client_version,
    report_issue_install_method, report_issue_os, report_issue_url,
};
use gpui::TestAppContext;

#[test]
fn report_issue_url_prefills_all_fields() {
    let url = report_issue_url(
        "20261015.1",
        Some("0.9.1"),
        "macOS 15.3.1 Sequoia, Apple Silicon",
        Some("Locally built app bundle"),
        Some("Disconnected: socket closed"),
    );
    assert_eq!(
        url,
        "https://github.com/penso/herdr-gpui/issues/new?template=bug_report.yml\
         &version=20261015.1\
         &daemon_version=0.9.1\
         &os=macOS+15.3.1+Sequoia%2C+Apple+Silicon\
         &install_method=Locally+built+app+bundle\
         &logs=Disconnected%3A+socket+closed"
    );
}

#[test]
fn report_issue_url_omits_optional_fields_when_absent() {
    let url = report_issue_url("dev", None, "Linux (Ubuntu 24.04), x86_64", None, None);
    assert_eq!(
        url,
        "https://github.com/penso/herdr-gpui/issues/new?template=bug_report.yml\
         &version=dev\
         &os=Linux+%28Ubuntu+24.04%29%2C+x86_64"
    );
    assert!(!url.contains("daemon_version="));
    assert!(!url.contains("install_method="));
    assert!(!url.contains("logs="));
}

#[test]
fn report_issue_url_encodes_plus_in_version() {
    let url = report_issue_url("20260920.1+abc", None, "macOS, Apple Silicon", None, None);
    assert!(url.contains("version=20260920.1%2Babc"));
}

#[test]
fn report_issue_url_bounds_large_logs() {
    let large_log = "error ".repeat(300);
    let url = report_issue_url("dev", None, "macOS, Apple Silicon", None, Some(&large_log));
    assert!(url.contains("logs="));
    // Must contain ellipsis
    assert!(url.contains("%E2%80%A6"));
    // Entire URL stays well bounded
    assert!(url.len() < 3000);
}

#[test]
fn report_issue_os_returns_valid_description() {
    let os = report_issue_os();
    assert!(!os.is_empty());
    assert!(os.contains(','));
    if cfg!(target_os = "macos") {
        assert!(os.contains("macOS"));
        assert!(
            os.contains("M1 Pro")
                || os.contains("Apple Silicon")
                || os.contains("Intel")
                || os.contains("MacBook")
        );
    } else if cfg!(target_os = "linux") {
        assert!(os.contains("Linux"));
    } else if cfg!(target_os = "windows") {
        assert!(os.contains("Windows"));
    }
}

#[test]
fn clean_cpu_brand_strips_apple_and_intel_noise() {
    assert_eq!(clean_cpu_brand("Apple M1 Pro"), "M1 Pro");
    assert_eq!(clean_cpu_brand("Apple M2 Max"), "M2 Max");
    assert_eq!(
        clean_cpu_brand("Intel(R) Core(TM) i7-9750H CPU @ 2.60GHz"),
        "Intel Core i7-9750H"
    );
    assert_eq!(
        clean_cpu_brand("AMD Ryzen 7 7800X3D"),
        "AMD Ryzen 7 7800X3D"
    );
}

#[test]
fn format_mac_model_converts_screen_sizes() {
    assert_eq!(
        format_mac_model("MacBook Pro (14-inch, 2021)"),
        "14\" MacBook Pro"
    );
    assert_eq!(
        format_mac_model("MacBook Air (15-inch, M2, 2023)"),
        "15\" MacBook Air"
    );
    assert_eq!(format_mac_model("iMac (24-inch, M1, 2021)"), "24\" iMac");
    assert_eq!(format_mac_model("Mac mini (2023)"), "Mac mini (2023)");
    assert_eq!(format_mac_model("MacBookPro18,3"), "MacBookPro18,3");
}

#[test]
fn extract_ioreg_string_handles_quotes_and_brackets() {
    assert_eq!(
        extract_ioreg_string("      \"product-name\" = <\"MacBook Pro (14-inch, 2021)\">"),
        Some("MacBook Pro (14-inch, 2021)".to_owned())
    );
    assert_eq!(
        extract_ioreg_string("      \"product-name\" = \"MacBook Pro (14-inch, 2021)\""),
        Some("MacBook Pro (14-inch, 2021)".to_owned())
    );
    assert_eq!(extract_ioreg_string("invalid line"), None);
}

#[test]
fn report_issue_client_version_embeds_commit_or_release() {
    let version = report_issue_client_version();
    assert!(!version.is_empty());
    if crate::RELEASE_BUILD {
        assert_eq!(version, crate::APP_VERSION);
    } else if let Some(commit) = crate::constants::BUILD_COMMIT.filter(|c| !c.is_empty()) {
        assert_eq!(version, format!("dev ({commit})"));
    } else {
        assert_eq!(version, "dev");
    }
}

#[test]
fn report_issue_install_method_detects_cargo_or_bundle() {
    // Under cargo test, the current executable lives in target/debug
    let method = report_issue_install_method();
    assert_eq!(method, Some("cargo run / just run"));
}

#[gpui::test]
fn herdr_window_report_issue_url_integrates_state(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);

    // Default window without error or daemon reports client version and OS
    let default_url = view.read_with(cx, |this, _| this.report_issue_url());
    assert!(default_url.contains("template=bug_report.yml"));
    assert!(default_url.contains("version="));
    assert!(default_url.contains("os="));
    assert!(!default_url.contains("daemon_version="));
    assert!(!default_url.contains("logs="));

    // Connected daemon sets daemon_version
    view.update(cx, |this, _| {
        this.live.daemon_version = Some("0.9.5".into());
    });
    let with_daemon = view.read_with(cx, |this, _| this.report_issue_url());
    assert!(with_daemon.contains("daemon_version=0.9.5"));

    // Missing installation reports "none (not installed)" when version is absent
    view.update(cx, |this, _| {
        this.live.daemon_version = None;
        this.live.missing_installation = true;
    });
    let missing = view.read_with(cx, |this, _| this.report_issue_url());
    assert!(missing.contains("daemon_version=none+%28not+installed%29"));

    // An error populates logs
    view.update(cx, |this, _| {
        this.local_error = Some("Connection refused".into());
    });
    let with_error = view.read_with(cx, |this, _| this.report_issue_url());
    assert!(with_error.contains("logs="));
    assert!(with_error.contains("Connection+refused"));
}
