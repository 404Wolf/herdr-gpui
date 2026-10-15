#![allow(clippy::unwrap_used)]
use super::*;
use crate::endpoint::tests::host;

fn ids(view: &HerdrWindow) -> Vec<&str> {
    view.endpoints.iter().map(|e| e.id.as_str()).collect()
}

#[test]
fn unarranged_devices_rank_after_arranged_ones() {
    let order = ["ssh:b".to_owned(), "ssh:a".to_owned()];
    assert_eq!(rank(&order, "ssh:b"), 0);
    assert_eq!(rank(&order, "ssh:a"), 1);
    assert_eq!(rank(&order, "ssh:c"), 2);
    assert_eq!(rank(&order, "wsl:d"), 2);
}

#[test]
fn a_move_lands_beside_its_anchor() {
    let ids = ["a", "b", "c", "d"];
    assert_eq!(
        moved(&ids, "d", Beside::Before("b")).unwrap(),
        ["a", "d", "b", "c"]
    );
    assert_eq!(
        moved(&ids, "a", Beside::After("c")).unwrap(),
        ["b", "c", "a", "d"]
    );
    assert_eq!(
        moved(&ids, "a", Beside::After("d")).unwrap(),
        ["b", "c", "d", "a"]
    );
    assert_eq!(moved(&ids, "e", Beside::Before("a")), None);
    assert_eq!(moved(&ids, "a", Beside::Before("e")), None);
    assert_eq!(moved(&ids, "a", Beside::Before("a")), None);
}

#[gpui::test]
fn the_user_s_order_survives_catalog_reloads_and_keeps_the_selection(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, cx| {
        let catalog = || vec![host("a", false), host("b", false), host("c", false)];
        view.reconcile_catalog(catalog(), Vec::new(), cx);
        assert_eq!(ids(view), [LOCAL, "ssh:a", "ssh:b", "ssh:c"]);
        view.selected_endpoint = 1;

        assert!(view.move_device("ssh:a", Beside::After("ssh:c")));
        assert_eq!(ids(view), [LOCAL, "ssh:b", "ssh:c", "ssh:a"]);
        assert_eq!(view.device_order, ["ssh:b", "ssh:c", "ssh:a"]);
        // The selection follows its device rather than its old place.
        assert_eq!(view.selected_endpoint, 3);
        // Dropping a device where it already is changes nothing.
        assert!(!view.move_device("ssh:a", Beside::After("ssh:c")));
        // Local stays first.
        assert!(!view.move_device(LOCAL, Beside::After("ssh:a")));

        // A reload keeps the arrangement and lists a new device last.
        let mut more = catalog();
        more.insert(0, host("new", false));
        view.reconcile_catalog(more, Vec::new(), cx);
        assert_eq!(ids(view), [LOCAL, "ssh:b", "ssh:c", "ssh:a", "ssh:new"]);
        // The disabled fixtures cannot stay selected through a reload.
        view.selected_endpoint = 3;

        // A stored order arriving after the catalog rearranges the list.
        view.device_order = vec!["ssh:new".into(), "ssh:c".into()];
        view.arrange_devices();
        assert_eq!(ids(view), [LOCAL, "ssh:new", "ssh:c", "ssh:b", "ssh:a"]);
        assert_eq!(view.selected_endpoint, 4);
    });
}
