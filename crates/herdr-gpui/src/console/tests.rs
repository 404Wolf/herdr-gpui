use super::*;

#[test]
fn the_console_copy_finds_the_app_one_directory_up() {
    let root = Path::new("Programs").join("Herdr UI");
    let app = root.join("herdr-gpui.exe");
    let cli = root.join("bin").join("herdr-gpui.exe");
    assert_eq!(app_beside(&cli), Some(app.clone()));
    assert_eq!(cli_beside(&app), Some(cli));
    // Windows paths compare without case.
    assert_eq!(
        app_beside(&root.join("BIN").join("herdr-gpui.exe")),
        Some(app)
    );
}

#[test]
fn an_executable_outside_bin_opens_the_app_itself() {
    assert_eq!(
        app_beside(&Path::new("target").join("release").join("herdr-gpui.exe")),
        None
    );
    assert_eq!(app_beside(Path::new("herdr-gpui.exe")), None);
}
