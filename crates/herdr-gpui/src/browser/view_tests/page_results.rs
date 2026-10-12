use super::*;
use crate::browser::{LocalFile, TabId};

const SENT: &str = r#"{"kind":"send","summary":"Picked B","data":{"variant":"B"}}"#;

fn opened(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext, location: Location) -> TabId {
    let tab_scope = view.read_with(cx, |view, _| scope(&view.endpoints[0]));
    cx.update(|_, cx| {
        Store::update(cx, |store| {
            store.open(tab_scope, "w0", Some(location), Some("w0:p1".into()))
        })
        .unwrap()
    })
}

fn post(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext, id: TabId, body: &str) {
    cx.update(|window, cx| view.update(cx, |view, cx| view.page_posted(id, body, window, cx)));
}

fn listed(view: &Entity<HerdrWindow>, cx: &mut VisualTestContext, id: TabId) -> usize {
    view.read_with(cx, |view, _| view.browser.annotations.listed(id).len())
}

#[gpui::test]
fn a_local_pages_result_waits_in_its_notes_without_annotating(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx);
    let file = LocalFile::new(std::path::Path::new("/srv/mockups/index.html"), None).unwrap();
    let local = opened(&view, cx, Location::Local { file });
    post(&view, cx, local, SENT);
    // Queued as a note the user still sends, with the panel open.
    assert_eq!(listed(&view, cx, local), 1);
    view.read_with(cx, |view, _| {
        assert_eq!(view.browser.annotations.queued(local), 1);
        assert!(view.browser.annotations.open(local));
    });
    // Picking again changes the unsent answer instead of adding one.
    post(
        &view,
        cx,
        local,
        r#"{"kind":"send","summary":"Picked C","data":{"variant":"C"}}"#,
    );
    assert_eq!(listed(&view, cx, local), 1);
    // A web page cannot send results, however it posts.
    let web = opened(&view, cx, url("http://localhost:3000/"));
    post(&view, cx, web, SENT);
    assert_eq!(listed(&view, cx, web), 0);
}
