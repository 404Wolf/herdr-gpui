//! The activity bar's Pull requests and Issues sections: the focused
//! workspace's repository, as an orchestrator view the window owns, drawn
//! across the window in place of its terminals on the list the section
//! names. A view is made the first time a workspace is shown and kept while
//! that workspace exists, so switching back finds it as it was left.

use super::{List, OrchestratorView, ViewId};
use crate::{HerdrWindow, browser::OrchestratorRepo, sidebar::ActivityPanel};
use gpui::{prelude::*, *};

impl HerdrWindow {
    /// Keeps the shown section on the focused workspace's view, making it
    /// when the workspace has none and the shown host can run the
    /// orchestrator's scripts. Called from the tick and when a section is
    /// picked; a new view, or a new section, gets its list picked here.
    pub(crate) fn sync_activity_section(&mut self, cx: &mut Context<Self>) {
        let Some(list) = self.activity_section().and_then(list_of) else {
            return;
        };
        let workspace = self.browser_key().map(|(_, workspace)| workspace);
        let asked = self
            .activity
            .section_repo
            .clone()
            .filter(|(of, _)| workspace.as_ref() == Some(of))
            .map(|(_, repo)| repo);
        let repo = asked
            .or_else(|| {
                self.focused_checkout()
                    .and_then(|path| OrchestratorRepo::new(path).ok())
            })
            .filter(|_| self.orchestrator_host());
        let id = workspace
            .zip(repo)
            .map(|(workspace, repo)| self.orchestrator_for(&workspace, repo, cx));
        let changed = id != self.activity.section_view || self.activity.section_list != Some(list);
        self.activity.section_view = id;
        let Some(id) = id else {
            return;
        };
        if changed && let Some(view) = self.orchestrator_view(id).cloned() {
            view.update(cx, |view, cx| view.show_list(list, cx));
            self.activity.section_list = Some(list);
            cx.notify();
        }
    }

    /// Shows the focused workspace's issues.
    pub(crate) fn open_orchestrator(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, workspace)) = self.browser_key() else {
            self.show_flash(crate::window::Flash::warning("Open a workspace first"), cx);
            return;
        };
        self.open_orchestrator_in(workspace, None, window, cx);
    }

    /// Shows the issues of workspace `workspace`, listing the repository of
    /// `checkout` when given, else of its focused pane. Another workspace
    /// than the focused one is switched to first.
    pub(crate) fn open_orchestrator_in(
        &mut self,
        workspace: String,
        checkout: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The sections are the bar's: without it there is nowhere to show them.
        if !self.activity_bar_shown() {
            self.show_flash(
                crate::window::Flash::warning(
                    "Pull requests and issues show in the activity bar, which is turned off",
                ),
                cx,
            );
            return;
        }
        if !self.orchestrator_host() {
            self.show_flash(
                crate::window::Flash::warning("Pull requests and issues need a local or SSH host"),
                cx,
            );
            return;
        }
        if let Some(repo) = checkout.and_then(|path| OrchestratorRepo::new(path).ok()) {
            self.activity.section_repo = Some((workspace.clone(), repo));
        }
        self.dismiss_menu(window, cx);
        if self
            .browser_key()
            .is_none_or(|(_, focused)| focused != workspace)
        {
            self.navigate(crate::NavigationTarget::Workspace(&workspace), cx);
        }
        self.show_activity_panel(ActivityPanel::Issues, window, cx);
    }

    /// Back to Workspaces after the section's view opened something that
    /// shows there, such as a run's pane or a page, so it is not left hidden
    /// behind the section. Another view's events leave the section alone.
    pub(crate) fn leave_activity_section(&mut self, id: ViewId, cx: &mut Context<Self>) {
        if self.activity_section().and_then(list_of).is_none()
            || self.activity.section_view != Some(id)
        {
            return;
        }
        self.activity.pick(ActivityPanel::Workspaces);
        self.save_chrome();
        cx.notify();
    }

    fn section_view(&self) -> Option<Entity<OrchestratorView>> {
        self.orchestrator_view(self.activity.section_view?).cloned()
    }

    /// Gives the section's view the keyboard.
    pub(crate) fn focus_activity_section(&self, window: &mut Window, cx: &mut App) {
        let Some(view) = self.section_view() else {
            return;
        };
        let focus = view.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
    }

    /// The shown Pull requests or Issues section across the window's body.
    pub(crate) fn render_orchestrator_section(&self) -> Option<AnyElement> {
        let section = div()
            .id("activity-section")
            .debug_selector(|| "activity-section".into())
            .flex()
            .flex_1()
            .size_full()
            .min_h_0()
            .min_w_0();
        if let Some(view) = self.section_view() {
            return Some(section.child(view).into_any_element());
        }
        let theme = &self.theme;
        Some(
            section
                .flex_col()
                .items_center()
                .justify_center()
                .text_color(rgb(theme.muted))
                .child(
                    div()
                        .debug_selector(|| "activity-section-empty".into())
                        .child(self.orchestrator_unavailable()),
                )
                .into_any_element(),
        )
    }

    /// Why a section has no view to show; while one is being made, that it is.
    fn orchestrator_unavailable(&self) -> &'static str {
        if !self.orchestrator_host() {
            "Pull requests and issues need a local or SSH host"
        } else if self.browser_key().is_none() {
            "Open a workspace first"
        } else if self
            .focused_checkout()
            .and_then(|path| OrchestratorRepo::new(path).ok())
            .is_none()
        {
            "This workspace has no folder to read"
        } else {
            "Opening the repository…"
        }
    }
}

/// The list a section shows, for the sections that show one.
pub(crate) fn list_of(section: ActivityPanel) -> Option<List> {
    match section {
        ActivityPanel::Issues => Some(List::Issues),
        ActivityPanel::PullRequests => Some(List::PullRequests),
        ActivityPanel::Overview | ActivityPanel::Workspaces | ActivityPanel::Usage => None,
    }
}
