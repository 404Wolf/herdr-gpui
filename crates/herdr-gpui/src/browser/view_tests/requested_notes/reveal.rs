use super::*;
use crate::{browser::Shown, controls::Command};

fn shown(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext) -> Vec<Shown> {
    draw(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        view.group_slots()
            .into_iter()
            .map(|slot| view.group_shown(slot.id, cx))
            .collect()
    })
}

/// Notes sent from a page bring the agent's tab back over it, so the user
/// sees the agent take them.
#[gpui::test]
fn delivered_notes_bring_the_agent_into_view(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    with_idle_agent(&view, cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.command(Command::NewBrowserTab, window, cx)
        })
    });
    let page = crate::browser::TabId::test(0);
    assert_eq!(shown(&view, cx), [Shown::Page(page)]);

    assert_eq!(deliver(&view, cx, None, "w0:p1"), Some(NotesTo::Agent));
    assert_eq!(shown(&view, cx), [Shown::Terminal]);
}

/// Notes the agent does not get leave the page where it is.
#[gpui::test]
fn kept_notes_leave_the_page_shown(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    with_idle_agent(&view, cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            Arc::make_mut(view.live.snapshot.as_mut().unwrap())
                .agents
                .clear();
            view.command(Command::NewBrowserTab, window, cx);
        })
    });
    let page = crate::browser::TabId::test(0);

    assert_eq!(deliver(&view, cx, None, "w0:p1"), Some(NotesTo::Kept));
    assert_eq!(shown(&view, cx), [Shown::Page(page)]);
}
