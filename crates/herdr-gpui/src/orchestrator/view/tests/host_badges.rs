use super::*;
use crate::orchestrator::{
    Request, Timing,
    view::{Look, OrchestratorView},
};
use gpui::{TestAppContext, px, size};
use std::sync::Arc;

#[gpui::test]
fn run_host_badges_stay_inside_their_column(cx: &mut TestAppContext) {
    for font_size in [14., 24.] {
        for host in [
            Some("build-machine-with-a-long-hostname.example.ts.net"),
            Some("构建服务器.example.ts.net"),
            Some("dev"),
            None,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let request = Request {
                target: herdr_client::ConnectTarget::Local,
                checkout: dir.path().to_string_lossy().into_owned(),
                workspace_id: "w1".into(),
                token: None,
                data_root: Some(dir.path().join("data")),
                timing: Timing::default(),
            };
            let config = crate::config::Config::default();
            let mut ui = config.ui.clone();
            ui.size = font_size;
            let look = Look {
                theme: crate::config::Theme::default(),
                ui,
                mono: config.terminal.clone(),
            };
            let (view, cx) = cx.add_window_view(|_, cx| OrchestratorView::new(request, look, cx));
            let issue = item(
                Provider::Github,
                "22",
                "Signed-link login",
                "2026-10-04T00:00:00Z",
            );
            let mut run = run("r1", &issue, RunState::Failed, "2026-10-04T01:00:00Z");
            let workspace = run.workspace.as_mut().unwrap();
            workspace.host = host.map(str::to_owned);
            workspace.branch =
                "agent/22-web-signed-link-login-with-an-extra-long-branch-name".into();
            view.update(cx, |view, cx| {
                view.snapshot.items = Arc::new(vec![issue]);
                view.recorded_runs = Arc::new(vec![run]);
                view.tab = Tab::Runs;
                view.preview_open = false;
                view.refresh_rows();
                cx.notify();
            });

            for width in [1100., 760.] {
                cx.simulate_resize(size(px(width), px(500.)));
                cx.run_until_parked();
                let column = cx.debug_bounds("orchestrator-run-host-column-1").unwrap();
                let badge = cx.debug_bounds("orchestrator-run-host-1").unwrap();
                let branch = cx.debug_bounds("orchestrator-run-branch-1").unwrap();
                let context = format!("host={host:?}, font={font_size}, width={width}");
                assert!(badge.size.width > px(0.), "{context}: {badge:?}");
                assert!(
                    badge.left() >= column.left(),
                    "{context}: {badge:?} {column:?}"
                );
                assert!(
                    badge.right() <= column.right(),
                    "{context}: badge {badge:?} overflows column {column:?}"
                );
                assert!(
                    badge.right() < branch.left(),
                    "{context}: badge {badge:?} overlaps branch {branch:?}"
                );
                if host.is_none() || host == Some("dev") {
                    assert!(
                        badge.size.width < column.size.width,
                        "{context}: short badges should keep their natural width"
                    );
                }
            }
        }
    }
}
