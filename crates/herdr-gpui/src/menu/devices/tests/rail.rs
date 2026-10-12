use super::*;
use crate::{cloud::CloudProvider, menu::devices::rail as frame};
use gpui::{Entity, Modifiers, VisualTestContext, px, size};

fn open(cx: &mut gpui::TestAppContext) -> (Entity<HerdrWindow>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.simulate_resize(size(px(900.), px(700.)));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            // Not configured from the environment either.
            view.config.coder.url = None;
            #[cfg(feature = "daytona")]
            {
                view.config.daytona.api_url = None;
            }
            view.open_add_device(window, cx);
        });
    });
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let target = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(target, Modifiers::default());
    draw(cx);
}

fn page(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> Option<Page> {
    cx.update(|_, cx| view.read(cx).menu.page)
}

fn configured_elsewhere() -> bool {
    std::env::var_os("HERDR_CODER_URL").is_some()
        || std::env::var_os("HERDR_DAYTONA_API_URL").is_some()
}

#[gpui::test]
fn every_provider_sits_beside_ssh_and_says_whether_it_is_set_up(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(cx);
    if crate::cloud::unavailable().is_some() {
        // Nothing offers to create a device that could never connect.
        assert!(cx.debug_bounds("add-device-rail").is_none());
        return;
    }
    assert!(cx.debug_bounds("add-device-source-host").is_some());
    for provider in CloudProvider::ALL {
        let id = match provider {
            #[cfg(feature = "coder")]
            CloudProvider::Coder => "add-device-source-coder",
            #[cfg(feature = "daytona")]
            CloudProvider::Daytona => "add-device-source-daytona",
            #[cfg(feature = "railway")]
            CloudProvider::Railway => "add-device-source-railway",
        };
        assert!(cx.debug_bounds(id).is_some(), "{provider:?} is listed");
    }
    if configured_elsewhere() {
        return;
    }
    #[cfg(feature = "coder")]
    {
        assert!(cx.debug_bounds("add-device-source-coder-ready").is_none());
        // Unconfigured, Coder's pane points to Settings.
        click(cx, "add-device-source-coder");
        assert_eq!(page(&view, cx), Some(Page::AddCloud(CloudProvider::Coder)));
        assert!(cx.debug_bounds("cloud-setup-settings").is_some());

        cx.update(|_, cx| {
            view.update(cx, |view, _| {
                view.config.coder.url = Some("https://coder.example.com".into());
            })
        });
        draw(cx);
        assert!(cx.debug_bounds("add-device-source-coder-ready").is_some());
    }
}

#[gpui::test]
fn switching_sources_keeps_what_the_ssh_form_holds(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(cx);
    if crate::cloud::unavailable().is_some() {
        return;
    }
    cx.update(|_, cx| {
        let target = view.read(cx).menu.device_setup.as_ref().unwrap().fields[0].clone();
        target.update(cx, |field, cx| field.set_text_selected("me@build-box", cx));
    });
    let provider = CloudProvider::ALL[CloudProvider::ALL.len() - 1];
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_source(frame::Source::Cloud(provider), window, cx)
        })
    });
    draw(cx);
    assert_ne!(page(&view, cx), Some(Page::AddDevice));
    click(cx, "add-device-source-host");
    assert_eq!(page(&view, cx), Some(Page::AddDevice));
    cx.update(|window, cx| {
        let setup = view.read(cx).menu.device_setup.as_ref().unwrap();
        assert_eq!(setup.fields[0].read(cx).text(), "me@build-box");
        assert!(setup.fields[0].read(cx).focus.is_focused(window));
    });
}

#[gpui::test]
fn a_settings_only_provider_opens_its_settings(cx: &mut gpui::TestAppContext) {
    let (view, cx) = open(cx);
    if crate::cloud::unavailable().is_some() {
        return;
    }
    #[cfg(feature = "railway")]
    {
        click(cx, "add-device-source-railway");
        assert_eq!(
            page(&view, cx),
            Some(Page::AddCloud(CloudProvider::Railway))
        );
        cx.simulate_keystrokes("escape");
        assert_eq!(page(&view, cx), None, "Escape closes the dialog");
    }
    let _ = view;
}

#[test]
fn the_picker_names_cloud_machines_only_when_offered() {
    assert_ne!(
        frame::add_device_offer(true),
        frame::add_device_offer(false)
    );
    assert!(frame::add_device_offer(true).contains("cloud"));
    assert!(!frame::add_device_offer(false).contains("cloud"));
}

#[gpui::test]
fn switching_sources_never_resizes_the_dialog(cx: &mut gpui::TestAppContext) {
    let (_view, cx) = open(cx);
    if crate::cloud::unavailable().is_some() {
        return;
    }
    let ssh = cx.debug_bounds("menu-panel").unwrap();
    for provider in CloudProvider::ALL {
        let id = match provider {
            #[cfg(feature = "coder")]
            CloudProvider::Coder => "add-device-source-coder",
            #[cfg(feature = "daytona")]
            CloudProvider::Daytona => "add-device-source-daytona",
            #[cfg(feature = "railway")]
            CloudProvider::Railway => "add-device-source-railway",
        };
        click(cx, id);
        assert_eq!(cx.debug_bounds("menu-panel").unwrap(), ssh, "{provider:?}");
    }
    click(cx, "add-device-source-host");
    assert_eq!(cx.debug_bounds("menu-panel").unwrap(), ssh);
    // The fixed height still leaves the dialog inside a short window.
    cx.simulate_resize(size(px(900.), px(400.)));
    draw(cx);
    assert!(cx.debug_bounds("menu-panel").unwrap().bottom() <= px(400.));
}
