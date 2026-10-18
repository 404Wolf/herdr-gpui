//! A section with nothing to list says what it is for.
use crate::orchestrator::{
    Request, Timing,
    view::{Look, OrchestratorView, rows::Tab},
};
use gpui::TestAppContext;

#[gpui::test]
fn an_empty_section_explains_itself(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let config = crate::config::Config::default();
    let look = Look {
        theme: crate::config::Theme::default(),
        ui: config.ui.clone(),
        mono: config.terminal.clone(),
    };
    let request = Request {
        target: herdr_client::ConnectTarget::Local,
        checkout: dir.path().join("app").to_string_lossy().into_owned(),
        workspace_id: "app".into(),
        token: None,
        data_root: Some(dir.path().join("data")),
        timing: Timing::default(),
    };
    let (view, cx) = cx.add_window_view(|_, cx| OrchestratorView::with_repo(request, look, cx));
    cx.simulate_resize(gpui::size(gpui::px(1100.), gpui::px(600.)));
    for tab in [Tab::Issues, Tab::PullRequests] {
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                view.repos.clear();
                view.tab = tab;
                view.refresh_rows();
                cx.notify();
            })
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("orchestrator-empty").is_some());
        assert!(cx.debug_bounds("orchestrator-list").is_some());
    }
}
