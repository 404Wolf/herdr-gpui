use super::{file_handlers::hyperlinked, *};
use crate::menu::Page;

/// A window on a daemon without link methods, showing `notes.md` as an
/// explicit hyperlink to `target`.
pub(super) fn linked<'a>(
    target: &str,
    cx: &'a mut gpui::TestAppContext,
) -> (
    Entity<HerdrWindow>,
    &'a mut VisualTestContext,
    Point<Pixels>,
) {
    let peer = MockPeer::new();
    let (view, cx) = window(&peer, false, cx);
    view.update(cx, |view, _| view.live.surface = Some(hyperlinked(target)));
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    let link = at(&view, cx, 2, 0);
    (view, cx, link)
}

#[gpui::test]
fn a_mail_hyperlink_is_underlined_and_opened_only_with_the_link_modifier(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx, link) = linked("mailto:name@example.com", cx);
    // A pointer that only passes over shows nothing.
    cx.simulate_mouse_move(link, None, Modifiers::default());
    view.read_with(cx, |view, _| {
        assert!(view.hovered_local_link().is_none());
        assert!(!view.terminal_link_hovered(link, Modifiers::default()));
    });
    // A well-formed address is verified as soon as the modifier is held.
    cx.simulate_mouse_move(link, None, Modifiers::secondary_key());
    view.read_with(cx, |view, _| {
        let hovered = view.hovered_local_link().unwrap();
        assert_eq!((hovered.row, hovered.columns.clone()), (0, 0..8));
        assert!(view.terminal_link_hovered(link, Modifiers::secondary_key()));
    });
    cx.simulate_click(link, Modifiers::default());
    assert!(cx.opened_url().is_none());
    cx.simulate_click(link, Modifiers::secondary_key());
    assert_eq!(cx.opened_url().as_deref(), Some("mailto:name@example.com"));
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
}

#[gpui::test]
fn an_editor_hyperlink_opens_without_asking(cx: &mut gpui::TestAppContext) {
    let (_, cx, link) = linked("vscode://file/tmp/notes.md", cx);
    cx.simulate_click(link, Modifiers::secondary_key());
    assert_eq!(
        cx.opened_url().as_deref(),
        Some("vscode://file/tmp/notes.md")
    );
}

#[gpui::test]
fn any_other_scheme_opens_only_after_the_user_confirms_it(cx: &mut gpui::TestAppContext) {
    let (view, cx, link) = linked("ssh://example.com", cx);
    cx.simulate_click(link, Modifiers::secondary_key());
    assert!(cx.opened_url().is_none());
    view.read_with(cx, |view, _| {
        assert_eq!(view.menu.page, Some(Page::OpenLink))
    });
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    assert!(cx.debug_bounds("open-link-url").is_some());

    // "No" opens nothing and closes the question.
    let no = cx.debug_bounds("open-link-no").unwrap().center();
    cx.simulate_click(no, Modifiers::default());
    assert!(cx.opened_url().is_none());
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));

    // "Yes" opens the address that was shown.
    cx.simulate_click(link, Modifiers::secondary_key());
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    let yes = cx.debug_bounds("open-link-yes").unwrap().center();
    cx.simulate_click(yes, Modifiers::default());
    assert_eq!(cx.opened_url().as_deref(), Some("ssh://example.com"));
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
}

#[gpui::test]
fn the_question_answers_to_enter_and_escape(cx: &mut gpui::TestAppContext) {
    let (view, cx, link) = linked("ssh://example.com", cx);
    cx.simulate_click(link, Modifiers::secondary_key());
    cx.simulate_keystrokes("escape");
    assert!(cx.opened_url().is_none());
    view.read_with(cx, |view, _| assert!(view.menu.page.is_none()));
    cx.simulate_click(link, Modifiers::secondary_key());
    cx.simulate_keystrokes("enter");
    assert_eq!(cx.opened_url().as_deref(), Some("ssh://example.com"));
}
