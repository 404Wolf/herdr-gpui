//! Browser tabs a control request asks for, through the Unix control socket.

use super::{scope, store};
use crate::{
    HerdrWindow, NavigationTarget,
    browser::{
        Store, TabId,
        groups::{GroupId, Pick},
    },
    control::{Opening, Placed, Placement, Target},
};
use gpui::*;

impl HerdrWindow {
    /// The endpoint and workspace a control request names, if this window
    /// shows it. `strict` requires the caller's own daemon; otherwise any
    /// endpoint showing the named workspace qualifies. Only the Unix control
    /// socket asks.
    fn browser_target(&self, target: &Target<'_>, strict: bool) -> Option<(usize, String)> {
        self.endpoints
            .iter()
            .enumerate()
            .find_map(|(index, endpoint)| {
                if strict {
                    let matches = match target.daemon {
                        Some(daemon) => {
                            endpoint.connection.target.socket_path().ok().as_deref() == Some(daemon)
                        }
                        None => index == self.selected_endpoint,
                    };
                    if !matches {
                        return None;
                    }
                } else if target.workspace.is_none() {
                    return None;
                }
                let live = if index == self.selected_endpoint {
                    &self.live
                } else {
                    &endpoint.live
                };
                let snapshot = live.snapshot.as_ref()?;
                let workspace = match target.workspace {
                    Some(id) => id,
                    None => snapshot.focused_workspace_id.as_deref()?,
                };
                snapshot
                    .workspaces
                    .iter()
                    .any(|candidate| candidate.workspace_id == workspace)
                    .then(|| (index, workspace.to_owned()))
            })
    }

    /// Opens the tab a control request asks for, if this window shows its
    /// workspace. `None` leaves the request to another window.
    pub(crate) fn open_requested_browser_tab(
        &mut self,
        target: &Target<'_>,
        strict: bool,
        opening: &Opening<'_>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Placed> {
        let (index, workspace) = self.browser_target(target, strict)?;
        let scope = scope(&self.endpoints[index]);
        // Splitting needs the workspace's groups on screen: a workspace the
        // window does not show yet gets an ordinary tab.
        let Opening {
            location,
            focus,
            placement,
        } = *opening;
        let shown = self.browser_key() == Some((scope.clone(), workspace.clone()));
        // An agent showing the same page again gets its tab back, reloaded,
        // rather than another tab for every revision.
        let origin = target.pane;
        let before =
            store(cx).and_then(|store| store.opened_before(&scope, &workspace, origin, location));
        let id = match before {
            Some(id) => {
                #[cfg(any(target_os = "macos", windows))]
                self.browser.pages.reload(id, cx);
                id
            }
            None => {
                let opened = Store::update(cx, |store| {
                    store.open(
                        scope,
                        &workspace,
                        Some(location.clone()),
                        origin.map(str::to_owned),
                    )
                });
                let Some(id) = opened else {
                    return Some(Placed::Full);
                };
                id
            }
        };
        let beside = placement == Placement::Beside
            && shown
            && self.show_beside(id, target.pane, window, cx);
        if !beside && focus {
            let focused = self
                .live
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.focused_workspace_id.as_deref());
            if index != self.selected_endpoint {
                let endpoint = self.endpoints[index].id.clone();
                self.navigate_endpoint(&endpoint, NavigationTarget::Workspace(&workspace), cx);
            } else if focused != Some(workspace.as_str()) {
                self.navigate(NavigationTarget::Workspace(&workspace), cx);
            }
            // Recorded against the workspace, so the tab shows once the
            // navigation lands even if it is still in flight.
            self.show_browser_tab(id, window, cx);
        }
        cx.notify();
        Some(Placed::Opened {
            workspace_id: workspace,
            placement: if beside {
                Placement::Beside
            } else {
                Placement::Tab
            },
        })
    }

    /// The group showing `pane`'s terminal: the one that picked its Herdr
    /// tab, or else the one holding the window's connection.
    fn terminal_group(&self, pane: Option<&str>) -> Option<GroupId> {
        let tab = pane.and_then(|pane| {
            let snapshot = self.live.snapshot.as_ref()?;
            let found = snapshot.panes.iter().find(|p| p.pane_id == pane)?;
            Some(Pick::Herdr(found.tab_id.clone()))
        });
        self.group_slots()
            .into_iter()
            .map(|slot| slot.id)
            .find(|group| tab.is_some() && self.group_pick(*group) == tab)
            .or_else(|| self.primary_group())
    }

    /// Shows page `id` in a group beside the caller's terminal: the group
    /// the page already belongs to, another existing group, or a new one
    /// split off to the terminal's right. The terminal's group stays in use
    /// and keeps the keyboard, so the user can go on typing to the agent.
    /// Returns whether the page is beside the terminal.
    fn show_beside(
        &mut self,
        id: TabId,
        pane: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(home) = self.terminal_group(pane) else {
            return false;
        };
        let pick = Pick::Page(id);
        let owner = self.ensure_layout().map(|layout| layout.owner(&pick));
        let slots = self.group_slots();
        let index = slots.iter().position(|slot| slot.id == home).unwrap_or(0);
        let neighbour = slots
            .get(index + 1)
            .or_else(|| index.checked_sub(1).and_then(|before| slots.get(before)))
            .map(|slot| slot.id);
        let group = match owner.filter(|owner| *owner != home).or(neighbour) {
            Some(group) => group,
            None => {
                self.split_group(home, window, cx);
                match self.active_group().filter(|group| *group != home) {
                    Some(group) => group,
                    None => return false,
                }
            }
        };
        self.move_tab_to_group(pick, group, window, cx);
        self.activate_group(home, window, cx);
        true
    }

    /// Reloads this window's pages for `tabs`, as an agent asks after
    /// editing a page it showed.
    pub(crate) fn reload_browser_tabs(&mut self, tabs: &[TabId], cx: &mut Context<Self>) {
        #[cfg(any(target_os = "macos", windows))]
        for id in tabs {
            self.browser.pages.reload(*id, cx);
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = (tabs, cx);
    }
}
