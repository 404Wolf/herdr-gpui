use super::*;

/// An automatic removal of `w4`'s checkout for merged pull request #8,
/// waiting on the daemon with no dialog open.
fn automatic_removal(view: &mut HerdrWindow) {
    view.live.status = crate::state::ConnectionStatus::Connected;
    view.removal = Some(super::super::Removal {
        endpoint: (
            view.selection_epoch,
            view.endpoints[view.selected_endpoint].generation,
        ),
        boot_id: view.live.snapshot.as_ref().unwrap().boot_id.clone(),
        workspace: "w4".into(),
        pending: Some("auto".into()),
        force: false,
        merged: Some(8),
    });
}

#[gpui::test]
fn automatic_removal_never_arms_a_force_removal(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(sidebar::layout_tests::fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            automatic_removal(view);
            view.live.dialog_response = Some((
                "auto".into(),
                Some(Ok(serde_json::json!({"error": {
                    "code": "dirty_worktree_requires_force",
                    "message": "modified or untracked files"
                }}))),
            ));
            view.update_workspace_dialog(window, cx);
            // The refusal is reported, and no force record is left for a
            // later dialog to pick up.
            assert!(view.removal.is_none());
            assert!(
                view.local_error
                    .as_deref()
                    .is_some_and(|error| error.contains("dirty_worktree_requires_force"))
            );
            assert!(view.flash.is_none());
        })
    });
}

#[gpui::test]
fn automatic_removal_reports_success_with_its_pull_request(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(sidebar::layout_tests::fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            automatic_removal(view);
            view.live.dialog_response = Some((
                "auto".into(),
                Some(Ok(serde_json::json!({"result": {
                    "type": "worktree_removed", "workspace_id": "w4"
                }}))),
            ));
            view.update_workspace_dialog(window, cx);
            assert!(view.removal.is_none() && view.local_error.is_none());
            assert_eq!(
                view.flash.as_ref().map(|(flash, _)| flash),
                Some(&crate::window::Flash::success(
                    "Removed the worktree of merged pull request #8"
                ))
            );
        })
    });
}

#[gpui::test]
fn focused_merged_worktree_asks_with_the_delete_dialog(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(sidebar::layout_tests::fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.live.status = crate::state::ConnectionStatus::Connected;
            view.ask_merged_removal("w4", 8, window, cx);
            assert_eq!(
                view.menu.page,
                Some(super::super::Page::Dialog(WorkspaceAction::DeleteWorktree))
            );
            assert_eq!(view.menu.target.as_ref().unwrap().id, "w4");
            assert_eq!(view.menu.deletion.as_ref().unwrap().merged, Some(8));
            // Asking sends nothing; the user confirms in the dialog.
            assert!(view.removal.is_none());
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("dialog-merged-pr").is_some());
}
