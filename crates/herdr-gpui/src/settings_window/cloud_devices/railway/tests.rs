#![allow(clippy::unwrap_used)]
use super::{
    super::super::{Section, SettingsWindow},
    Account, CloudProvider, KeySource, Overview, PendingKey, setup,
};
use crate::{
    cloud::SavedDevice,
    railway::{KeyDirectory, Source},
};
use gpui::{Entity, TestAppContext, VisualTestContext, px, size};
use std::path::PathBuf;

fn open(
    cx: &mut TestAppContext,
) -> (
    (Entity<SettingsWindow>, &mut VisualTestContext),
    gpui::WindowHandle<crate::HerdrWindow>,
) {
    let main = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    let weak = cx.update(|cx| main.update(cx, |_, _, cx| cx.weak_entity()).unwrap());
    let (view, cx) = cx.add_window_view(|window, cx| {
        let view = SettingsWindow::new(weak, cx);
        window.focus(&view.focus, cx);
        view
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.cloud_tab = CloudProvider::Railway;
            view.select_section(Section::CloudDevices, window, cx);
        })
    });
    ((view, cx), main)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
}

/// A key this app generated at an exact path (in the user's `~/.ssh`).
fn generated_at(path: &str) -> Source {
    Source::Generated {
        path: Some(path.into()),
    }
}

fn device(index: usize, source: Source, claimed: bool, key_present: bool) -> setup::Device {
    let id = format!("key-{index}");
    let (Source::Generated { path: Some(path) } | Source::Borrowed { path }) = &source else {
        panic!("tests give every key a path");
    };
    setup::Device {
        saved: SavedDevice {
            provider: CloudProvider::Railway,
            id: id.clone(),
            label: format!("A long Railway device label number {index} that must truncate"),
            account: crate::railway::ACCOUNT.into(),
            machine: format!("railway-box-{index}"),
            session: "default".into(),
            enabled: true,
        },
        key_path: Some(PathBuf::from(path)),
        // Only keys generated in `~/.ssh` stay when their device goes.
        key_kept: matches!(source, Source::Generated { .. }),
        record: Some(crate::railway::Record {
            attached: true,
            window_ends_by: Some(0),
            claimed,
            ..crate::railway::Record::unattached(&id, source)
        }),
        key_present,
    }
}

fn show(view: &Entity<SettingsWindow>, overview: Overview, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.railway_card.as_mut().unwrap().account = Account::Known(overview);
        })
    });
}

fn empty() -> Overview {
    Overview {
        devices: Vec::new(),
        pending_key: None,
        new_key_directory: KeyDirectory {
            path: PathBuf::from("/home/u/.ssh"),
            user: true,
        },
        detected: Vec::new(),
    }
}

#[gpui::test]
fn the_tab_offers_a_generated_key_and_explicit_install_consent_by_default(cx: &mut TestAppContext) {
    let ((view, cx), _main) = open(cx);
    cx.simulate_resize(size(px(960.), px(1400.)));
    draw(cx);
    if CloudProvider::ALL.len() > 1 {
        assert!(cx.debug_bounds("cloud-tab-railway").is_some());
    }
    for selector in [
        "cloud-railway-generate",
        "cloud-railway-card",
        "cloud-railway-key",
        "cloud-railway-source-generate",
        "cloud-railway-source-existing",
        "cloud-railway-install",
        "cloud-railway-create",
        "cloud-railway-devices",
    ] {
        assert!(cx.debug_bounds(selector).is_some(), "{selector}");
    }
    assert!(
        cx.debug_bounds("cloud-railway-browse").is_none(),
        "the path field belongs to the existing-key choice"
    );
    view.read_with(cx, |view, cx| {
        let card = view.railway_card.as_ref().unwrap();
        assert_eq!(card.source, KeySource::Generate);
        assert!(card.install && card.job.is_none());
        assert!(card.path.read(cx).text().is_empty());
    });
}

#[gpui::test]
fn choosing_an_existing_key_shows_its_path_browse_and_detected_keys(cx: &mut TestAppContext) {
    let ((view, cx), _main) = open(cx);
    show(
        &view,
        Overview {
            detected: vec![PathBuf::from("/home/u/.ssh/id_ed25519")],
            ..empty()
        },
        cx,
    );
    cx.simulate_resize(size(px(960.), px(1600.)));
    draw(cx);
    let existing = cx.debug_bounds("cloud-railway-source-existing").unwrap();
    cx.simulate_click(existing.center(), gpui::Modifiers::none());
    draw(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.railway_card.as_ref().unwrap().source,
            KeySource::Existing
        )
    });
    assert!(cx.debug_bounds("cloud-railway-browse").is_some());
    let detected = cx.debug_bounds("cloud-railway-detected-0").unwrap();
    cx.simulate_click(detected.center(), gpui::Modifiers::none());
    view.read_with(cx, |view, cx| {
        let card = view.railway_card.as_ref().unwrap();
        assert_eq!(card.path.read(cx).text(), "/home/u/.ssh/id_ed25519");
    });
    // Back to a generated key hides the path again.
    let generate = cx.debug_bounds("cloud-railway-source-generate").unwrap();
    cx.simulate_click(generate.center(), gpui::Modifiers::none());
    draw(cx);
    assert!(cx.debug_bounds("cloud-railway-browse").is_none());
}

#[gpui::test]
fn an_existing_key_choice_without_a_path_starts_nothing(cx: &mut TestAppContext) {
    let ((view, cx), main) = open(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.railway_card.as_mut().unwrap().source = KeySource::Existing;
            view.setup_railway_new(cx);
        })
    });
    view.read_with(cx, |view, _| {
        let message = view
            .railway_card
            .as_ref()
            .unwrap()
            .message
            .as_deref()
            .unwrap();
        assert!(message.contains("path"), "{message}");
    });
    cx.update(|_, cx| {
        main.update(cx, |main, _, _| assert!(main.cloud_jobs.is_empty()))
            .unwrap()
    });
}

#[gpui::test]
fn saved_devices_list_their_key_and_state_within_the_narrowest_width(cx: &mut TestAppContext) {
    let ((view, cx), _main) = open(cx);
    show(
        &view,
        Overview {
            devices: vec![
                device(0, generated_at("/home/u/.ssh/herdr-railway"), false, true),
                device(
                    1,
                    Source::Borrowed {
                        path: "/home/u/.ssh/id".into(),
                    },
                    true,
                    true,
                ),
                device(2, generated_at("/home/u/.ssh/two"), false, false),
            ],
            ..empty()
        },
        cx,
    );
    cx.simulate_resize(size(px(680.), px(2000.)));
    draw(cx);
    let devices = cx.debug_bounds("cloud-railway-devices").unwrap();
    assert!(devices.right() <= px(680.));
    for index in 0..3 {
        for action in ["reconnect", "remove"] {
            let selector: &'static str =
                Box::leak(format!("cloud-railway-{action}-{index}").into_boxed_str());
            let bounds = cx.debug_bounds(selector).unwrap();
            assert!(bounds.left() >= devices.left() && bounds.right() <= devices.right());
        }
    }
    assert!(cx.debug_bounds("cloud-railway-claimed-0").is_some());
    assert!(
        cx.debug_bounds("cloud-railway-claimed-1").is_none(),
        "a claimed VM cannot be marked claimed again"
    );
}

#[test]
fn device_rows_say_what_key_is_attached_and_when_it_is_unusable() {
    let line = |device: &setup::Device, now| super::describe(device, now);
    let generated = device(0, generated_at("/home/u/.ssh/herdr-railway"), false, true);
    assert_eq!(
        line(&generated, 0),
        "railway-box-0 · generated key /home/u/.ssh/herdr-railway"
    );
    assert!(line(&generated, 1).ends_with("build window probably ended"));
    let borrowed = device(1, Source::Borrowed { path: "/k".into() }, false, true);
    assert!(line(&borrowed, 0).contains("key /k"));
    assert!(
        line(&device(1, generated_at("/home/u/.ssh/one"), true, true), 99).ends_with("claimed")
    );
    assert!(
        line(
            &device(2, generated_at("/home/u/.ssh/two"), false, false),
            0
        )
        .ends_with("key file missing")
    );
    let mut orphan = device(3, generated_at("/k"), false, false);
    orphan.record = None;
    assert!(line(&orphan, 0).contains("no saved key"));
}

#[gpui::test]
fn a_new_key_shows_the_interactive_first_connection_and_copies_it(cx: &mut TestAppContext) {
    let ((view, cx), _main) = open(cx);
    cx.simulate_resize(size(px(960.), px(1600.)));
    draw(cx);
    // No key yet: the step offers to make one rather than invent a command.
    assert!(cx.debug_bounds("cloud-railway-generate").is_some());
    assert!(cx.debug_bounds("cloud-railway-first-use").is_none());

    show(
        &view,
        Overview {
            pending_key: Some(PendingKey {
                path: PathBuf::from("/state/railway-keys/key-abc"),
                fingerprint: Some("SHA256:abcdef".into()),
                kept: false,
            }),
            ..empty()
        },
        cx,
    );
    draw(cx);
    assert!(cx.debug_bounds("cloud-railway-generate").is_none());
    assert!(cx.debug_bounds("cloud-railway-first-use").is_some());
    let copy = cx.debug_bounds("cloud-railway-copy").unwrap();
    cx.simulate_click(copy.center(), gpui::Modifiers::none());
    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap();
    assert_eq!(
        copied,
        "ssh -F /dev/null -i '/state/railway-keys/key-abc' -o IdentitiesOnly=yes railway.new"
    );

    // An existing key's command follows the path typed for it, with `~` expanded.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let card = view.railway_card.as_mut().unwrap();
            card.source = KeySource::Existing;
            card.path.update(cx, |input, cx| {
                input.set_text_selected("/home/u/.ssh/id", cx)
            });
            cx.notify();
        })
    });
    draw(cx);
    let copy = cx.debug_bounds("cloud-railway-copy").unwrap();
    cx.simulate_click(copy.center(), gpui::Modifiers::none());
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap(),
        "ssh -F /dev/null -i '/home/u/.ssh/id' -o IdentitiesOnly=yes railway.new"
    );
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.railway_card
                .as_mut()
                .unwrap()
                .path
                .update(cx, |input, cx| input.clear(cx));
        })
    });
    draw(cx);
    assert!(
        cx.debug_bounds("cloud-railway-first-use").is_none(),
        "nothing to run before a key is chosen"
    );
}

#[gpui::test]
fn generating_asks_for_a_name_and_says_where_the_key_goes(cx: &mut TestAppContext) {
    let ((view, cx), _main) = open(cx);
    show(&view, empty(), cx);
    cx.simulate_resize(size(px(960.), px(1600.)));
    draw(cx);
    view.read_with(cx, |view, cx| {
        let card = view.railway_card.as_ref().unwrap();
        assert_eq!(card.name.read(cx).text(), "herdr-railway");
    });
    assert!(cx.debug_bounds("cloud-railway-generate").is_some());
    assert!(
        cx.debug_bounds("cloud-railway-pending").is_none()
            && cx.debug_bounds("cloud-railway-first-use").is_none(),
        "no command to run before there is a key"
    );
    // A name that is not one safe file name is explained and cannot be used.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let card = view.railway_card.as_mut().unwrap();
            card.name
                .update(cx, |input, cx| input.set_text_selected("../escape", cx));
            view.generate_railway_key(cx);
        })
    });
    view.read_with(cx, |view, _| {
        let card = view.railway_card.as_ref().unwrap();
        assert_eq!(card.message.as_deref(), Some(crate::railway::NAME_RULE));
        assert!(card.job.is_none(), "nothing ran for an unsafe name");
    });
}

#[test]
fn the_location_note_names_the_folder_and_what_removal_does() {
    let ssh = KeyDirectory {
        path: PathBuf::from("/home/u/.ssh"),
        user: true,
    };
    let note = super::location_note(&ssh, "herdr-railway");
    assert!(note.contains("/home/u/.ssh/herdr-railway") && note.contains("herdr-railway.pub"));
    assert!(note.contains("keeps them unless you delete"));
    let own = KeyDirectory {
        path: PathBuf::from("/state/herdr/gpui/railway-keys"),
        user: false,
    };
    let note = super::location_note(&own, "herdr-railway");
    assert!(note.contains("No ~/.ssh folder") && note.contains("railway-keys/herdr-railway"));
    assert!(note.contains("readable only by you") && note.contains("deletes them"));
}

#[gpui::test]
fn a_waiting_key_shows_its_path_fingerprint_and_a_way_to_discard_it(cx: &mut TestAppContext) {
    let ((view, cx), main) = open(cx);
    show(
        &view,
        Overview {
            pending_key: Some(PendingKey {
                path: PathBuf::from("/home/u/.ssh/herdr-railway"),
                fingerprint: Some("SHA256:abcdef".into()),
                kept: true,
            }),
            ..empty()
        },
        cx,
    );
    cx.simulate_resize(size(px(680.), px(2000.)));
    draw(cx);
    assert!(cx.debug_bounds("cloud-railway-pending").is_some());
    assert!(cx.debug_bounds("cloud-railway-discard").is_some());
    assert!(
        cx.debug_bounds("cloud-railway-generate").is_none(),
        "one key waits at a time; discard it to make another"
    );
    // Setting up with it is the next step, and the first connection stays the
    // user's own, shown as a command for exactly this key.
    assert!(cx.debug_bounds("cloud-railway-first-use").is_some());
    let key = cx.debug_bounds("cloud-railway-key").unwrap();
    assert!(key.right() <= px(680.));
    // Without a key waiting, setting up asks for one first and starts nothing.
    show(&view, empty(), cx);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| view.setup_railway_new(cx));
        main.update(cx, |main, _, _| assert!(main.cloud_jobs.is_empty()))
            .unwrap();
    });
    view.read_with(cx, |view, _| {
        let message = view.railway_card.as_ref().unwrap().message.as_deref();
        assert_eq!(message, Some("Generate a key first."));
    });
}

#[gpui::test]
fn only_keys_that_stay_offer_an_explicit_delete(cx: &mut TestAppContext) {
    let ((view, cx), _main) = open(cx);
    let mut theirs = device(
        1,
        Source::Borrowed {
            path: "/k/id".into(),
        },
        false,
        true,
    );
    theirs.key_kept = false;
    let mut own = device(2, generated_at("/state/railway-keys/key-2"), false, true);
    own.key_kept = false;
    show(
        &view,
        Overview {
            devices: vec![
                device(0, generated_at("/home/u/.ssh/herdr-railway"), false, true),
                theirs,
                own,
            ],
            ..empty()
        },
        cx,
    );
    cx.simulate_resize(size(px(680.), px(2000.)));
    draw(cx);
    assert!(
        cx.debug_bounds("cloud-railway-remove-key-0").is_some(),
        "a key in ~/.ssh stays on plain Remove, so deleting it is a separate action"
    );
    for index in [1, 2] {
        let selector: &'static str =
            Box::leak(format!("cloud-railway-remove-key-{index}").into_boxed_str());
        assert!(cx.debug_bounds(selector).is_none(), "{selector}");
    }
    let devices = cx.debug_bounds("cloud-railway-devices").unwrap();
    let both = cx.debug_bounds("cloud-railway-remove-key-0").unwrap();
    assert!(both.right() <= devices.right() && both.left() >= devices.left());
}
