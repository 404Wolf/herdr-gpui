//! A main checkout whose shell moved into another repository. Herdr keeps the
//! repository the workspace was opened in while its label follows the shell,
//! so a workspace labelled for the new directory would still branch the old
//! repository: the menu must not offer to, and a request must refuse.

use super::*;
use crate::menu::{
    workspace::{LEFT_CHECKOUT, NewWorktreeUnavailable},
    worktree_shortcut::{Unavailable, WorktreeShortcut},
};

/// The fixture's main checkout, moved to a sibling repository's directory.
fn moved() -> herdr_client::protocol::ClientShellSnapshot {
    let mut snapshot = sidebar::layout_tests::snapshot(7);
    let root = sidebar::layout_tests::REPO_KEY.trim_end_matches("/agent-launcher/.git");
    snapshot.workspaces[3].new_workspace_cwd = format!("{root}/wgsextract-cli");
    snapshot.workspaces[3].label = "wgsextract-cli".into();
    snapshot.focused_workspace_id = Some("w3".into());
    snapshot
}

#[test]
fn checkout_paths_follow_the_endpoint_shape() {
    use crate::worktree::left_checkout;
    let key = "/home/mac/src/herdr-gpui/.git";
    for cwd in [
        "/home/mac/src/herdr-gpui",
        "/home/mac/src/herdr-gpui/",
        "/home/mac/src/herdr-gpui/crates/herdr-gpui",
    ] {
        assert!(!left_checkout(key, cwd), "{cwd}");
    }
    for cwd in [
        "/home/mac/src/wgsextract-cli",
        "/home/mac/src/herdr-gpui-other",
        "/home/mac",
    ] {
        assert!(left_checkout(key, cwd), "{cwd}");
    }
    // A Windows endpoint may report either separator and any case.
    let windows = r"C:\Users\mac\src\herdr-gpui\.git";
    assert!(!left_checkout(
        windows,
        "c:/users/mac/src/Herdr-GPUI/crates"
    ));
    assert!(left_checkout(windows, r"C:\Users\mac\src\wgsextract-cli"));
    // Nothing is known to have moved without a directory or a `<root>/.git` key.
    assert!(!left_checkout(key, ""));
    assert!(!left_checkout("/srv/git/herdr-gpui.git", "/home/mac"));
    assert!(!left_checkout("/.git", "/home/mac"));
}

#[test]
fn a_moved_main_checkout_offers_no_worktrees() {
    let snapshot = moved();
    let target = WorkspaceTarget::new(&snapshot, &snapshot.workspaces[3]);
    assert!(target.left_checkout && !target.can_create());
    assert_eq!(
        WorkspaceTarget::for_new_worktree(&snapshot, &snapshot.workspaces[3]).err(),
        Some(NewWorktreeUnavailable::LeftCheckout)
    );
    assert_eq!(
        WorktreeShortcut::Open.target(&snapshot),
        Err(Unavailable::LeftCheckout)
    );
    assert_eq!(Unavailable::LeftCheckout.message(), LEFT_CHECKOUT);
    assert_eq!(
        NewWorktreeUnavailable::LeftCheckout.message(),
        LEFT_CHECKOUT
    );
}

/// A dialog opened before the shell moved refuses rather than branching the
/// repository its label no longer names.
#[test]
fn a_main_checkout_that_moves_after_the_dialog_opened_refuses() {
    let before = sidebar::layout_tests::snapshot(7);
    let target = WorkspaceTarget::for_new_worktree(&before, &before.workspaces[3]).unwrap();
    for action in [WorkspaceAction::NewWorktree, WorkspaceAction::OpenWorktree] {
        assert!(matches!(
            target.request(&moved(), action, "branch"),
            Err(crate::Error::WorkspaceRepositoryChanged)
        ));
    }
}

/// A linked checkout still names its repository, so it creates through its
/// main checkout even when that checkout's shell has wandered off.
#[test]
fn a_linked_checkout_creates_through_a_moved_main_checkout() {
    let snapshot = moved();
    let target = WorkspaceTarget::for_new_worktree(&snapshot, &snapshot.workspaces[4]).unwrap();
    assert_eq!(target.id, "w3");
    let (method, params) = target
        .request(&snapshot, WorkspaceAction::NewWorktree, "")
        .unwrap();
    assert_eq!(method, Method::WorktreeCreate);
    assert_eq!(params["base"], "refs/heads/worktree/sidebar-child");
}
