use super::{external::linked, *};

/// A directory of its own holding `notes.md`, removed when dropped.
struct Folder(std::path::PathBuf);

impl Folder {
    fn new() -> Self {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "hg-explicit-path-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("notes.md"), "# Notes\n").unwrap();
        Self(directory)
    }

    fn url(&self, name: &str) -> String {
        url::Url::from_file_path(self.0.join(name)).unwrap().into()
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Puts the pane in `folder`, with files opening in the system's
/// application, which the test observes.
fn enter(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext, folder: &Folder) {
    view.update(cx, |view, _| {
        view.config.open_files_in = crate::config::FileTarget::System;
        let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
        for pane in &mut snapshot.panes {
            pane.cwd = Some(folder.0.to_str().unwrap().into());
            pane.foreground_cwd = pane.cwd.clone();
        }
    });
}

/// The text of the message over the terminal.
fn flashed(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| {
        view.flash.as_ref().unwrap().0.text.to_string()
    })
}

fn not_found(path: &std::path::Path) -> String {
    crate::Error::LinkedFileMissing { path: path.into() }.to_string()
}

fn underlined(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.hovered_local_link().is_some())
}

#[gpui::test]
fn a_hyperlink_to_a_missing_file_is_a_link_whose_click_says_so(cx: &mut gpui::TestAppContext) {
    let folder = Folder::new();
    let missing = folder.url("gone.md");
    let (view, cx, link) = linked(&missing, cx);
    enter(&view, cx, &folder);

    // A pointer passing over shows nothing. With the link modifier held the
    // file is looked for: not found, the link keeps its dashes, and the
    // pointer still shows a link, as over any explicit hyperlink.
    cx.simulate_mouse_move(link, None, Modifiers::default());
    cx.run_until_parked();
    assert!(!underlined(&view, cx));
    view.read_with(cx, |view, _| {
        assert!(!view.terminal_link_hovered(link, Modifiers::default()));
    });
    cx.simulate_mouse_move(link, None, Modifiers::secondary_key());
    cx.run_until_parked();
    assert!(!underlined(&view, cx));
    view.read_with(cx, |view, _| {
        assert!(view.terminal_link_hovered(link, Modifiers::secondary_key()));
    });

    cx.simulate_click(link, Modifiers::default());
    cx.run_until_parked();
    assert!(cx.opened_url().is_none());
    view.read_with(cx, |view, _| assert!(view.flash.is_none()));
    cx.simulate_click(link, Modifiers::secondary_key());
    cx.run_until_parked();
    assert!(cx.opened_url().is_none());
    assert_eq!(flashed(&view, cx), not_found(&folder.0.join("gone.md")));
}

// These targets are Unix paths.
#[cfg(unix)]
#[gpui::test]
fn a_hyperlink_without_a_scheme_resolves_against_the_pane_directory(cx: &mut gpui::TestAppContext) {
    let folder = Folder::new();
    let expected = url::Url::from_file_path(folder.0.canonicalize().unwrap().join("notes.md"))
        .unwrap()
        .to_string();
    let absolute = folder.0.join("notes.md").to_str().unwrap().to_owned();
    let parent = format!(
        "../{}/notes.md",
        folder.0.file_name().unwrap().to_str().unwrap()
    );
    for target in [absolute.as_str(), "./notes.md", "notes.md", parent.as_str()] {
        let (view, cx, link) = linked(target, cx);
        enter(&view, cx, &folder);
        cx.simulate_mouse_move(link, None, Modifiers::default());
        cx.run_until_parked();
        assert!(!underlined(&view, cx), "{target}");
        cx.simulate_mouse_move(link, None, Modifiers::secondary_key());
        cx.run_until_parked();
        assert!(underlined(&view, cx), "{target}");
        cx.simulate_click(link, Modifiers::secondary_key());
        cx.run_until_parked();
        assert_eq!(
            cx.opened_url().as_deref(),
            Some(expected.as_str()),
            "{target}"
        );
    }
}

/// A relative target to a file that is not there names where it looked.
#[gpui::test]
fn a_missing_relative_hyperlink_names_the_path_it_resolved(cx: &mut gpui::TestAppContext) {
    let folder = Folder::new();
    let (view, cx, link) = linked("./gone.md", cx);
    enter(&view, cx, &folder);
    cx.simulate_click(link, Modifiers::secondary_key());
    cx.run_until_parked();
    assert!(cx.opened_url().is_none());
    assert_eq!(flashed(&view, cx), not_found(&folder.0.join("./gone.md")));
}

#[gpui::test]
fn a_hyperlink_to_another_hosts_file_only_says_so(cx: &mut gpui::TestAppContext) {
    let (view, cx, link) = linked("file://other-host/etc/hosts", cx);
    // Nothing can verify it, so it keeps its dashes; the pointer shows a link.
    cx.simulate_mouse_move(link, None, Modifiers::secondary_key());
    cx.run_until_parked();
    assert!(!underlined(&view, cx));
    view.read_with(cx, |view, _| {
        assert!(view.terminal_link_hovered(link, Modifiers::secondary_key()));
    });
    cx.simulate_click(link, Modifiers::secondary_key());
    cx.run_until_parked();
    assert!(cx.opened_url().is_none());
    assert_eq!(
        flashed(&view, cx),
        crate::Error::LinkOnAnotherHost {
            host: "other-host".into()
        }
        .to_string()
    );
}

#[gpui::test]
fn a_hyperlink_with_no_valid_address_only_says_so(cx: &mut gpui::TestAppContext) {
    let (view, cx, link) = linked("https://", cx);
    cx.simulate_mouse_move(link, None, Modifiers::secondary_key());
    cx.run_until_parked();
    assert!(!underlined(&view, cx));
    view.read_with(cx, |view, _| {
        assert!(view.terminal_link_hovered(link, Modifiers::secondary_key()));
    });
    cx.simulate_click(link, Modifiers::secondary_key());
    cx.run_until_parked();
    assert!(cx.opened_url().is_none());
    assert_eq!(flashed(&view, cx), crate::Error::MalformedLink.to_string());
}
