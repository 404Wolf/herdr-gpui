//! The activity bar's Orchestrator, Pull requests, and Issues sections: the
//! focused workspace's orchestrator tab, drawn across the window in place of
//! its terminals, on the list the section names. The tab is opened as the
//! workspace menu opens it when the workspace has none, so its events,
//! restore, and detaching work as they do for any orchestrator tab.

use super::{List, OrchestratorView};
use crate::{
    HerdrWindow,
    browser::{Location, OrchestratorRepo, Store, TabId},
    sidebar::ActivityPanel,
};
use gpui::{prelude::*, *};

impl HerdrWindow {
    /// Keeps the shown section on the focused workspace's orchestrator tab,
    /// opening one when the workspace has none and the shown host can run
    /// the orchestrator's scripts. Called from the tick and when a section
    /// is picked; a new tab, or a new section, gets its list picked here.
    pub(crate) fn sync_activity_section(&mut self, cx: &mut Context<Self>) {
        let Some(list) = self.activity_section().map(list_of) else {
            return;
        };
        let Some((scope, workspace)) = self.browser_key() else {
            self.activity.section_tab = None;
            return;
        };
        // A tab moved into a window of its own leaves the store but stays the
        // workspace's orchestrator, so the section offers that window rather
        // than opening a second one.
        let target = &self.endpoints[self.selected_endpoint].connection.target;
        let current = self.activity.section_tab.filter(|id| {
            cx.try_global::<Store>()
                .and_then(|store| store.get(*id))
                .is_some_and(|tab| tab.scope == scope && tab.workspace_id == workspace)
                || self.detached_orchestrators.get(id).is_some_and(|detached| {
                    let view = detached.orchestrator.view.read(cx);
                    view.workspace_id() == workspace && view.target() == target
                })
        });
        let id = current.or_else(|| {
            let existing = cx.try_global::<Store>().and_then(|store| {
                store
                    .in_workspace(&scope, &workspace)
                    .find(|tab| matches!(tab.location, Some(Location::Orchestrator { .. })))
                    .map(|tab| tab.id)
            });
            existing.or_else(|| {
                let repo = self
                    .focused_checkout()
                    .filter(|_| self.orchestrator_host())
                    .and_then(|path| OrchestratorRepo::new(path).ok())?;
                Store::update(cx, |store| {
                    store.open(
                        scope,
                        &workspace,
                        Some(Location::Orchestrator { repo }),
                        None,
                    )
                })
            })
        });
        let changed = id != self.activity.section_tab || self.activity.section_list != Some(list);
        self.activity.section_tab = id;
        let Some(id) = id else {
            return;
        };
        self.ensure_orchestrator(id, cx);
        if changed && let Some(view) = self.section_view(id) {
            view.update(cx, |view, cx| view.show_list(list, cx));
            self.activity.section_list = Some(list);
            cx.notify();
        }
    }

    /// Back to Workspaces after the section's orchestrator opened something
    /// that shows there, such as a run's pane or a page, so it is not left
    /// hidden behind the section. Another orchestrator's events leave the
    /// section alone.
    pub(crate) fn leave_activity_section(&mut self, id: TabId, cx: &mut Context<Self>) {
        if self.activity_section().is_none() || self.activity.section_tab != Some(id) {
            return;
        }
        self.activity.pick(ActivityPanel::Workspaces);
        self.save_chrome();
        cx.notify();
    }

    /// The view of tab `id`, in the window or one of its own.
    fn section_view(&self, id: TabId) -> Option<Entity<OrchestratorView>> {
        self.orchestrators
            .get(&id)
            .map(|orchestrator| orchestrator.view.clone())
            .or_else(|| {
                self.detached_orchestrators
                    .get(&id)
                    .map(|detached| detached.orchestrator.view.clone())
            })
    }

    /// Gives the section's view the keyboard, as opening its tab would.
    pub(crate) fn focus_activity_section(&self, window: &mut Window, cx: &mut App) {
        let Some(view) = self
            .activity
            .section_tab
            .and_then(|id| self.orchestrators.get(&id))
            .map(|orchestrator| orchestrator.view.clone())
        else {
            return;
        };
        let focus = view.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
    }

    /// The shown section across the window's body, or `None` in Workspaces.
    pub(crate) fn render_activity_section(&self) -> Option<AnyElement> {
        self.activity_section()?;
        let section = div()
            .id("activity-section")
            .debug_selector(|| "activity-section".into())
            .flex()
            .flex_1()
            .size_full()
            .min_h_0()
            .min_w_0();
        let id = self.activity.section_tab;
        if let Some(orchestrator) = id.and_then(|id| self.orchestrators.get(&id)) {
            return Some(section.child(orchestrator.view.clone()).into_any_element());
        }
        let theme = &self.theme;
        let (text, window) = match id.and_then(|id| self.detached_orchestrators.get(&id)) {
            Some(detached) => (
                "The orchestrator is in a window of its own",
                Some(detached.window),
            ),
            None => (self.orchestrator_unavailable(), None),
        };
        let hover = theme.active;
        Some(
            section
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .text_color(rgb(theme.muted))
                .child(
                    div()
                        .debug_selector(|| "activity-section-empty".into())
                        .child(text),
                )
                .when_some(window, |section, window| {
                    section.child(
                        div()
                            .id("activity-section-window")
                            .debug_selector(|| "activity-section-window".into())
                            .px(px(10.))
                            .py(px(4.))
                            .rounded(px(crate::config::corners::CONTROL))
                            .border_1()
                            .border_color(rgb(theme.active))
                            .text_color(rgb(theme.foreground))
                            .cursor_pointer()
                            .hover(move |style| style.bg(rgb(hover)))
                            .child("Show its window")
                            .on_click(move |_, _, cx| {
                                let _ = window.update(cx, |_, window, _| window.activate_window());
                            }),
                    )
                })
                .into_any_element(),
        )
    }

    /// Why a section has no orchestrator to show, in the words opening one
    /// from a workspace uses; while one is being opened, that it is.
    fn orchestrator_unavailable(&self) -> &'static str {
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
            "Opening the orchestrator…"
        }
    }
}

/// The orchestrator's list a section shows.
fn list_of(section: ActivityPanel) -> List {
    match section {
        ActivityPanel::Issues => List::Issues,
        ActivityPanel::PullRequests => List::PullRequests,
        ActivityPanel::Workspaces | ActivityPanel::Orchestrator => List::Runs,
    }
}
