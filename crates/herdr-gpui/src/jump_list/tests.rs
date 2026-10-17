#![allow(clippy::unwrap_used)]

use super::*;

fn recent(path: &str) -> Recent {
    Recent {
        path: PathBuf::from(path),
        label: path.trim_start_matches("C:/").into(),
    }
}

#[test]
fn the_latest_checkout_leads_and_the_list_is_bounded() {
    let mut list = Vec::new();
    for index in 0..MAX_RECENT + 3 {
        remember(&mut list, recent(&format!("C:/repo{index}")));
    }
    assert_eq!(list.len(), MAX_RECENT);
    assert_eq!(list[0], recent(&format!("C:/repo{}", MAX_RECENT + 2)));
    // Focusing one again moves it up instead of listing it twice.
    let again = list[3].clone();
    remember(&mut list, again.clone());
    assert_eq!(list[0], again);
    assert_eq!(list.iter().filter(|entry| **entry == again).count(), 1);
}

#[test]
fn entries_are_links_with_safe_short_titles() {
    let mut long = recent("C:/repo");
    long.label = format!("evil\u{202e}{}", "x".repeat(200));
    let entries = entries(
        &[long],
        &[
            ("local".into(), "Local".into()),
            ("ssh:box".into(), "Box".into()),
        ],
    );
    assert_eq!(
        entries.recent[0].link,
        Link::Open {
            path: "C:/repo".into()
        }
    );
    assert!(!entries.recent[0].title.contains('\u{202e}'));
    assert!(entries.recent[0].title.chars().count() <= MAX_LABEL);
    assert_eq!(
        entries.devices,
        vec![
            Item {
                title: "Show Local".into(),
                link: Link::Device { id: "local".into() }
            },
            Item {
                title: "Show Box".into(),
                link: Link::Device {
                    id: "ssh:box".into()
                }
            },
        ]
    );
}

#[gpui::test]
fn only_a_focused_local_git_checkout_is_remembered(cx: &mut gpui::TestAppContext) {
    let handle = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    handle
        .update(cx, |view, _, _| {
            let mut snapshot = crate::sidebar::layout_tests::snapshot(3);
            snapshot.focused_workspace_id = Some("w0".into());
            snapshot
                .panes
                .push(herdr_client::protocol::ClientShellPane {
                    pane_id: "p0".into(),
                    workspace_id: "w0".into(),
                    tab_id: "t0".into(),
                    label: None,
                    cwd: Some(r"C:\src\herdr".into()),
                    foreground_cwd: None,
                    focused: true,
                    right_click_passthrough: false,
                });
            view.live.snapshot = Some(Arc::new(snapshot.clone()));
            assert_eq!(
                focused_checkout(view),
                Some(Recent {
                    path: r"C:\src\herdr".into(),
                    label: "herdr".into()
                })
            );
            // Not a Git checkout: nothing a link could open.
            snapshot.workspaces[0].branch = None;
            view.live.snapshot = Some(Arc::new(snapshot));
            assert_eq!(focused_checkout(view), None);
        })
        .unwrap();
}

#[test]
fn windows_with_different_device_orders_publish_the_same_list() {
    let local = ("local".to_owned(), "Local".to_owned());
    let boat = ("ssh:boat".to_owned(), "boat".to_owned());
    let alpha = ("ssh:alpha".to_owned(), "Alpha".to_owned());
    let one = entries(&[], &[boat.clone(), local.clone(), alpha.clone()]);
    let other = entries(&[], &[alpha, boat, local]);
    assert_eq!(one, other);
    let titles: Vec<&str> = one.devices.iter().map(|item| item.title.as_str()).collect();
    assert_eq!(titles, ["Show Local", "Show Alpha", "Show boat"]);
}
