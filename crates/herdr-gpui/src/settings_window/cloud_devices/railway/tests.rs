#![allow(clippy::unwrap_used)]
use super::{
    super::super::{Section, SettingsWindow},
    CloudProvider, KeySource, Overview, setup,
};
use crate::{cloud::SavedDevice, railway::KeyDirectory};
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

fn device(index: usize) -> setup::Device {
    setup::Device {
        saved: SavedDevice {
            provider: CloudProvider::Railway,
            id: format!("key-{index}"),
            label: format!("A long Railway device label number {index} that must truncate"),
            account: crate::railway::ACCOUNT.into(),
            machine: format!("railway-box-{index}"),
            session: "default".into(),
            enabled: true,
        },
        key_path: Some(PathBuf::from(format!("/home/u/.ssh/key-{index}"))),
        key_present: true,
    }
}

fn show(view: &Entity<SettingsWindow>, devices: Vec<setup::Device>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.railway_card.as_mut().unwrap().overview = Some(Overview {
                devices,
                pending_key: None,
                new_key_directory: KeyDirectory {
                    path: PathBuf::from("/home/u/.ssh"),
                    user: true,
                },
            });
        })
    });
}

#[gpui::test]
fn the_key_choices_and_install_consent_fit_the_narrowest_width(cx: &mut TestAppContext) {
    let ((view, cx), _main) = open(cx);
    show(&view, Vec::new(), cx);
    cx.simulate_resize(size(px(680.), px(1600.)));
    draw(cx);
    for selector in [
        "cloud-railway-card",
        "cloud-railway-generate",
        "cloud-railway-source-existing",
        "cloud-railway-install",
        "cloud-railway-create",
        "cloud-railway-devices",
    ] {
        let bounds = cx.debug_bounds(selector);
        assert!(bounds.is_some_and(|b| b.right() <= px(680.)), "{selector}");
    }
    assert!(cx.debug_bounds("cloud-railway-browse").is_none());
    let existing = cx.debug_bounds("cloud-railway-source-existing").unwrap();
    cx.simulate_click(existing.center(), gpui::Modifiers::none());
    draw(cx);
    view.read_with(cx, |view, _| {
        let card = view.railway_card.as_ref().unwrap();
        assert_eq!(card.source, KeySource::Existing);
        assert!(
            card.install,
            "installing needs the user's switch, on by default"
        );
    });
    assert!(cx.debug_bounds("cloud-railway-browse").is_some());
    assert!(cx.debug_bounds("cloud-railway-generate").is_none());
}

#[gpui::test]
fn setup_without_a_usable_key_starts_nothing(cx: &mut TestAppContext) {
    let ((view, cx), main) = open(cx);
    show(&view, Vec::new(), cx);
    let message = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.railway_card.as_ref().unwrap().message.clone().unwrap()
        })
    };
    cx.update(|_, cx| view.update(cx, |view, cx| view.setup_railway(cx)));
    assert_eq!(message(cx), "Generate a key first.");
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let card = view.railway_card.as_mut().unwrap();
            card.source = KeySource::Existing;
            view.setup_railway(cx);
        })
    });
    assert!(message(cx).contains("path"));
    // A name that is not one safe file name is explained, not generated.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let card = view.railway_card.as_mut().unwrap();
            card.name
                .update(cx, |input, cx| input.set_text_selected("../escape", cx));
            view.generate_railway_key(cx);
        })
    });
    assert_eq!(message(cx), crate::railway::NAME_RULE);
    view.read_with(cx, |view, _| {
        assert!(view.railway_card.as_ref().unwrap().job.is_none());
    });
    cx.update(|_, cx| {
        main.update(cx, |main, _, _| assert!(main.cloud_jobs.is_empty()))
            .unwrap()
    });
}

#[gpui::test]
fn saved_devices_only_offer_remove(cx: &mut TestAppContext) {
    let ((view, cx), _main) = open(cx);
    show(&view, vec![device(0), device(1)], cx);
    cx.simulate_resize(size(px(680.), px(2000.)));
    draw(cx);
    let devices = cx.debug_bounds("cloud-railway-devices").unwrap();
    for index in 0..2 {
        let remove = cx
            .debug_bounds(Box::leak(
                format!("cloud-railway-remove-{index}").into_boxed_str(),
            ))
            .unwrap();
        assert!(remove.left() >= devices.left() && remove.right() <= devices.right());
    }
    for selector in ["cloud-railway-reconnect-0", "cloud-railway-claimed-0"] {
        assert!(cx.debug_bounds(selector).is_none(), "{selector}");
    }
}
