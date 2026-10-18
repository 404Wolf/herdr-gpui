#![allow(clippy::unwrap_used)]
use super::*;
use crate::settings_window::tests::skill_fixture;
use core::prelude::v1::test;
use gpui::TestAppContext;

#[gpui::test]
fn px0_address_edits_survive_reload_and_invalid_input_is_not_saved(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(skill_fixture);
    view.update(cx, |view, cx| {
        view.open_px0_settings(cx);
        let input = view.px0.input.clone().unwrap();
        input.update(cx, |input, cx| {
            input.set_text_selected("http://example.com/", cx)
        });
        view.apply_px0_address(cx);
        assert!(view.px0.invalid);
        assert!(view.config.px0.url.is_none());
        view.sync_px0_field(cx);
        assert_eq!(input.read(cx).text(), "http://example.com/");
        view.px0.invalid = false;
        input.update(cx, |input, cx| input.set_text_selected("", cx));
        view.config.px0.url = Some(WebUrl::try_from("http://localhost:7777/").unwrap());
        view.sync_px0_field(cx);
        assert_eq!(input.read(cx).text(), "http://localhost:7777/");
    });
}
