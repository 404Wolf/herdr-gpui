//! One item listed by two repositories, as one GitHub repository checked out
//! on two hosts lists every issue twice under the same key: each copy's
//! selection, page, and dispatch go to its own repository, never the first.
use super::*;
use crate::orchestrator::{
    Access, DispatchRequest, Request, Timing,
    view::{Look, OrchestratorView, RepoSpec, repo::Listed, rows::Tab},
};
use crate::teleport::AgentKind;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use std::sync::Arc;

fn spec(dir: &std::path::Path, name: &str) -> RepoSpec {
    RepoSpec {
        key: name.into(),
        host: name.into(),
        request: Request {
            target: herdr_client::ConnectTarget::Local,
            checkout: dir.join(name).to_string_lossy().into_owned(),
            workspace_id: name.into(),
            token: None,
            data_root: Some(dir.join("data")),
            timing: Timing::default(),
        },
    }
}

fn listed(repo: &str, key: &str) -> Listed {
    Listed {
        repo: repo.into(),
        key: key.into(),
    }
}

/// A view of repositories `laptop` and `server`, both listing issue #377.
fn two_copies<'a>(
    dir: &std::path::Path,
    cx: &'a mut TestAppContext,
) -> (Entity<OrchestratorView>, &'a mut VisualTestContext, String) {
    let config = crate::config::Config::default();
    let look = Look {
        theme: crate::config::Theme::default(),
        ui: config.ui.clone(),
        mono: config.terminal.clone(),
    };
    let (view, cx) = cx.add_window_view(|_, cx| OrchestratorView::new(look, cx));
    cx.simulate_resize(gpui::size(gpui::px(1100.), gpui::px(600.)));
    let issue = item(Provider::Github, "377", "Icons", "2026-10-04T00:00:00Z");
    let key = issue.key.canonical();
    let specs = vec![spec(dir, "laptop"), spec(dir, "server")];
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_repos(specs, cx);
            for repo in &mut view.repos {
                repo.snapshot.items = Arc::new(vec![issue.clone()]);
                repo.snapshot.access = Some(Access::ReadWrite);
            }
            view.tab = Tab::Issues;
            view.refresh_rows();
            cx.notify();
        })
    });
    cx.run_until_parked();
    (view, cx, key)
}

fn workspace(view: &Entity<OrchestratorView>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| view.read(cx).workspace_id().map(str::to_owned))
}

#[gpui::test]
fn start_agent_on_either_copy_dispatches_from_that_copys_repository(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx, key) = two_copies(dir.path(), cx);
    for repo in ["server", "laptop"] {
        let selector: &'static str =
            Box::leak(format!("orchestrator-start-{repo}-{key}").into_boxed_str());
        let at = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector}"))
            .center();
        cx.simulate_click(at, Modifiers::none());
        cx.run_until_parked();
        let dialog = cx.update(|_, cx| view.read(cx).dialog.as_ref().map(|d| d.item.clone()));
        assert_eq!(dialog, Some(listed(repo, &key)));
        // The dialog's request is made from its repository's workspace, and
        // its action goes to that repository's worker.
        assert_eq!(workspace(&view, cx).as_deref(), Some(repo));
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.close_overlays(window, cx);
            })
        });
    }
}

#[gpui::test]
fn the_keyboard_steps_through_both_copies(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx, key) = two_copies(dir.path(), cx);
    let selected = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| view.read(cx).selected.clone().map(|listed| listed.repo))
    };
    let keys = cx.update(|_, cx| view.read(cx).keys());
    assert_eq!(keys, [listed("laptop", &key), listed("server", &key)]);
    assert_eq!(selected(cx).as_deref(), Some("laptop"));
    for (by, repo) in [(1, "server"), (1, "server"), (-1, "laptop")] {
        cx.update(|_, cx| view.update(cx, |view, cx| view.move_selection(by, cx)));
        assert_eq!(selected(cx).as_deref(), Some(repo));
        assert_eq!(workspace(&view, cx).as_deref(), Some(repo));
    }
}

#[gpui::test]
fn an_open_item_keeps_its_repository_while_another_row_is_selected(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx, key) = two_copies(dir.path(), cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_detail(listed("server", &key), cx);
            view.selected = Some(listed("laptop", &key));
            // The page's runs, messages, and Stop go to the open copy.
            assert_eq!(view.workspace_id(), Some("server"));
            let open = view.detail.as_ref().unwrap().listed.clone();
            view.open_dispatch(open, window, cx);
            assert_eq!(
                view.dialog.as_ref().map(|dialog| dialog.item.repo.as_str()),
                Some("server")
            );
        })
    });
}

#[gpui::test]
fn a_dispatch_whose_repository_went_starts_nothing_elsewhere(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (view, cx, key) = two_copies(dir.path(), cx);
    let laptop = spec(dir.path(), "laptop");
    let notice = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_dispatch(listed("server", &key), window, cx);
            // The server's workspace closes while the dialog is open; the
            // laptop still lists the same item under the same key.
            view.set_repos(vec![laptop], cx);
            assert_eq!(view.workspace_id(), None);
            view.submit_dispatch(window, cx);
            assert!(view.dialog.is_none());
            view.notice_text()
        })
    });
    let gone = crate::orchestrator::Error::RepositoryGone.to_string();
    assert_eq!(notice.as_deref(), Some(gone.as_str()));
    // A dispatch set up for another host comes back for a repository that
    // went meanwhile: it is refused, not handed to the one still listed.
    let request = DispatchRequest {
        item_key: key,
        kind: AgentKind::Claude,
        model: None,
        prompt: "Fix it".into(),
        branch: "agent/icons-1234abcd".into(),
        workspace_id: "server".into(),
        base: None,
        elsewhere: None,
    };
    let notice = cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.notice = None;
            view.dispatch_elsewhere("server", request);
            view.notice_text()
        })
    });
    assert_eq!(notice.as_deref(), Some(gone.as_str()));
}
