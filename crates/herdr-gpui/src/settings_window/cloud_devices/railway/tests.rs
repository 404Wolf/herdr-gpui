#![allow(clippy::unwrap_used)]
use super::{
    super::super::{Section, SettingsWindow},
    Account, CloudProvider, KeySource, Overview, setup,
};
use crate::{cloud::SavedDevice, railway::Source};
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

fn device(index: usize, source: Source, claimed: bool, key_present: bool) -> setup::Device {
    let id = format!("key-{index}");
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
        record: Some(crate::railway::Record {
            id,
            source,
            attached: true,
            window_ends_by: Some(0),
            claimed,
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
                device(0, Source::Generated, false, true),
                device(
                    1,
                    Source::Borrowed {
                        path: "/home/u/.ssh/id".into(),
                    },
                    true,
                    true,
                ),
                device(2, Source::Generated, false, false),
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
    let generated = device(0, Source::Generated, false, true);
    assert_eq!(line(&generated, 0), "railway-box-0 · generated key");
    assert!(line(&generated, 1).ends_with("build window probably ended"));
    let borrowed = device(1, Source::Borrowed { path: "/k".into() }, false, true);
    assert!(line(&borrowed, 0).contains("key /k"));
    assert!(line(&device(1, Source::Generated, true, true), 99).ends_with("claimed"));
    assert!(line(&device(2, Source::Generated, false, false), 0).ends_with("key file missing"));
    let mut orphan = device(3, Source::Generated, false, false);
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
            pending_key: Some(PathBuf::from("/state/railway-keys/key-abc")),
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
