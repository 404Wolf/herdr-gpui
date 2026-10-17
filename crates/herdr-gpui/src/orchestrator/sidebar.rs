//! The orchestrator the activity bar's panels read when no tab shows one: a
//! view of the focused workspace's repository that no tab hosts, polled from
//! the window's tick like the tabs' views. It starts once a list panel is
//! shown, follows the focused workspace, and is handed to the tab when the
//! user opens one there, so the tab shows its lists at once.

use super::{OrchestratorView, Request, Timing};
use crate::{HerdrWindow, browser::OrchestratorRepo};
use gpui::{prelude::*, *};

/// A view no tab hosts, and what it was opened for.
pub(crate) struct SidebarOrchestrator {
    /// The device it reads through, by endpoint ID.
    endpoint: String,
    workspace: String,
    checkout: String,
    pub(crate) view: Entity<OrchestratorView>,
}

impl SidebarOrchestrator {
    pub(crate) fn workspace(&self) -> &str {
        &self.workspace
    }

    pub(crate) fn checkout(&self) -> &str {
        &self.checkout
    }
}

impl HerdrWindow {
    /// Opens, keeps, or drops the sidebar's own orchestrator view: wanted
    /// while the bar has shown a list panel, the shown host can run the
    /// orchestrator's scripts, and no tab already has a view of the focused
    /// workspace. Moving to another workspace or device opens a new one.
    pub(crate) fn sync_sidebar_orchestrator(&mut self, cx: &mut Context<Self>) {
        let wanted = self.sidebar_orchestrator_wanted(cx);
        let current = self
            .activity
            .orchestrator
            .as_ref()
            .map(|connected| (connected.endpoint.as_str(), connected.workspace.as_str()));
        if current
            == wanted
                .as_ref()
                .map(|(endpoint, workspace)| (endpoint.as_str(), workspace.as_str()))
        {
            return;
        }
        self.activity.orchestrator = None;
        let Some((endpoint, workspace)) = wanted else {
            cx.notify();
            return;
        };
        // The checkout is read once: following the pane's directory would
        // restart the worker on every `cd`.
        let Some(repo) = self
            .focused_checkout()
            .and_then(|path| OrchestratorRepo::new(path).ok())
        else {
            return;
        };
        let target = self.endpoints[self.selected_endpoint]
            .connection
            .target
            .clone();
        let (token, login) = self.github_account(&target);
        let request = Request {
            target,
            checkout: repo.checkout.clone(),
            workspace_id: workspace.clone(),
            token,
            data_root: None,
            timing: Timing::default(),
        };
        let look = self.look();
        let view = cx.new(|cx| {
            let mut view = OrchestratorView::new(request, look, cx);
            view.set_github(None, login, cx);
            view
        });
        self.activity.orchestrator = Some(SidebarOrchestrator {
            endpoint,
            workspace,
            checkout: repo.checkout,
            view,
        });
        cx.notify();
    }

    /// Why the panels have no orchestrator to read, in the words opening one
    /// from a workspace uses; while one is being opened, that it is.
    pub(crate) fn orchestrator_unavailable(&self) -> &'static str {
        if !self.orchestrator_host() {
            "The orchestrator needs a local or SSH host"
        } else if self.browser_key().is_none() {
            "Open a workspace first"
        } else if self
            .focused_checkout()
            .and_then(|path| OrchestratorRepo::new(path).ok())
            .is_none()
        {
            "This workspace has no folder to read"
        } else {
            "Connecting to the orchestrator…"
        }
    }

    /// The device and workspace the sidebar's view should read, if any.
    fn sidebar_orchestrator_wanted(&self, cx: &App) -> Option<(String, String)> {
        if !self.activity_bar_shown() || !self.activity.listed() || !self.orchestrator_host() {
            return None;
        }
        let (_, workspace) = self.browser_key()?;
        let target = &self.endpoints[self.selected_endpoint].connection.target;
        let hosted = self.orchestrators.values().any(|orchestrator| {
            let view = orchestrator.view.read(cx);
            view.workspace_id() == workspace && view.target() == target
        });
        (!hosted).then(|| (self.endpoints[self.selected_endpoint].id.clone(), workspace))
    }

    /// The sidebar's view, if it reads `checkout` of `workspace` on the shown
    /// device, given up to the tab being opened there.
    pub(super) fn take_sidebar_orchestrator(
        &mut self,
        workspace: &str,
        checkout: &str,
    ) -> Option<Entity<OrchestratorView>> {
        let endpoint = &self.endpoints[self.selected_endpoint].id;
        self.activity
            .orchestrator
            .take_if(|connected| {
                &connected.endpoint == endpoint
                    && connected.workspace == workspace
                    && connected.checkout == checkout
            })
            .map(|connected| connected.view)
    }
}
