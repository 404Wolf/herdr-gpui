use super::*;

#[gpui::test]
fn warning_does_not_block_saved_device_or_disappear_with_the_dialog(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let request = setup::Request::new("remote.test", "Remote", "").unwrap();
            open_form(view, Step::Saving(request, HostProbe::Running), cx);
            view.device_save_succeeded(setup::SaveOutcome::ExistingServer, cx);
            assert!(matches!(step(view), Step::Saved));
            assert!(view.menu.device_setup.as_ref().unwrap().error.is_none());
            view.poll_device_setup(window, cx);
            assert!(view.menu.device_setup.is_none());
            assert!(view.menu.page.is_none());
            assert!(view.focus.is_focused(window));
            assert_eq!(
                view.flash.as_ref().unwrap().0,
                crate::window::Flash::warning(
                    setup::SaveOutcome::ExistingServer.warning().unwrap()
                )
            );
        });
    });
}
