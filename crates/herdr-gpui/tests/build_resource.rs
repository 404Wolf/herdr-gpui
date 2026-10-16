//! The Windows resource script the build script writes, checked on every
//! platform: only a Windows host compiles and embeds it.
#![allow(clippy::unwrap_used)]

#[path = "../build_resource.rs"]
mod build_resource;

use build_resource::*;

#[test]
fn release_versions_split_the_date_across_two_fields() {
    assert_eq!(file_version("20261016.3"), Some([2026, 1016, 3, 0]));
    assert_eq!(file_version("20260101.65535"), Some([2026, 101, 65535, 0]));
    for invalid in [
        "",
        "dev",
        "20261016",
        "20261016.",
        "20261016.0",
        "20261016.03",
        "2026101.3",
        "202610161.3",
        "02610161.3",
        "20261016.3.1",
        "20261016.65536",
        "2026-10-16.3",
        "v20261016.3",
    ] {
        assert_eq!(file_version(invalid), None, "{invalid}");
    }
}

#[test]
fn script_names_the_product_and_carries_the_release_version() {
    let script = script(r"C:\src\assets\icons\windows\herdr.ico", Some("20261016.3")).unwrap();
    assert!(
        script.starts_with("1 ICON \"C:\\\\src\\\\assets\\\\icons\\\\windows\\\\herdr.ico\"\n")
    );
    for line in [
        "FILEVERSION 2026,1016,3,0",
        "PRODUCTVERSION 2026,1016,3,0",
        "VALUE \"FileDescription\", \"Herdr UI\"",
        "VALUE \"ProductName\", \"Herdr UI\"",
        "VALUE \"CompanyName\", \"Herdr UI\"",
        "VALUE \"FileVersion\", \"20261016.3\"",
        "VALUE \"ProductVersion\", \"20261016.3\"",
        "VALUE \"OriginalFilename\", \"herdr-gpui.exe\"",
    ] {
        assert!(script.lines().any(|l| l.trim() == line), "{line}\n{script}");
    }
}

#[test]
fn local_builds_are_dev_and_bad_release_versions_are_refused() {
    let script = script("C:/icon.ico", None).unwrap();
    assert!(script.contains("FILEVERSION 0,0,0,0\n"));
    assert!(script.contains("VALUE \"FileVersion\", \"dev\"\n"));
    assert_eq!(build_resource::script("C:/icon.ico", Some("dev")), None);
}
