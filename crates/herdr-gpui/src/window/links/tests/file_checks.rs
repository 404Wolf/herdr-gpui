//! When a printed path is read from disk: each time the user looks at it
//! with the link modifier, and not only once.

use super::{
    cases::{LINK, Sandbox, in_sandbox, showing},
    *,
};

/// A printed path that names nothing yet, and the window showing it.
fn printed<'a>(
    sandbox: &Sandbox,
    path: &str,
    peer: &MockPeer,
    cx: &'a mut gpui::TestAppContext,
) -> (Entity<HerdrWindow>, &'a mut VisualTestContext) {
    let text = sandbox.fill(&format!("{{work}}/{path}"));
    in_sandbox(sandbox, showing(&text, false), peer, false, cx)
}

#[gpui::test]
fn a_path_found_missing_is_checked_again_when_looked_at_again(cx: &mut gpui::TestAppContext) {
    let sandbox = Sandbox::new();
    let peer = MockPeer::new();
    let (view, cx) = printed(&sandbox, "target/built.rs", &peer, cx);
    let link = at(&view, cx, LINK.0, LINK.1);
    let elsewhere = at(&view, cx, LINK.0, LINK.1 + 3);
    let (none, held) = (Modifiers::default(), Modifiers::secondary_key());
    let shown = |view: &Entity<HerdrWindow>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.hovered_local_link().is_some())
    };

    cx.simulate_mouse_move(link, None, held);
    assert!(!shown(&view, cx), "not built yet");
    std::fs::create_dir(sandbox.work.join("target")).unwrap();
    std::fs::write(sandbox.work.join("target/built.rs"), "").unwrap();

    // Letting go of the modifier and holding it again looks again.
    cx.simulate_mouse_move(link, None, none);
    cx.simulate_mouse_move(link, None, held);
    assert!(shown(&view, cx), "after the modifier");

    std::fs::remove_file(sandbox.work.join("target/built.rs")).unwrap();
    // So does leaving the path and coming back with it held.
    cx.simulate_mouse_move(elsewhere, None, held);
    cx.simulate_mouse_move(link, None, held);
    assert!(!shown(&view, cx), "after leaving the path");
}
