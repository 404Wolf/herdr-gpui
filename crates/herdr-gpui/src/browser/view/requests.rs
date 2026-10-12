//! Browser tabs a control request asks for, through the Unix control socket.

use super::{scope, store};
use crate::{
    HerdrWindow, NavigationTarget,
    browser::{Location, Scope, Store, TabId},
    control::{Placed, Target},
};
use gpui::*;

impl HerdrWindow {
    /// The endpoint and workspace a control request names, if this window
    /// shows it. `strict` requires the caller's own daemon; otherwise any
    /// endpoint showing the named workspace qualifies. Only the Unix control
    /// socket asks.
    pub(crate) fn control_target(
        &self,
        target: &Target<'_>,
        strict: bool,
    ) -> Option<(usize, String)> {
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
        location: &Location,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Placed> {
        let (index, workspace) = self.control_target(target, strict)?;
        let scope = scope(&self.endpoints[index]);
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
        if focus {
            self.go_to_workspace(index, &workspace, cx);
            // Recorded against the workspace, so the tab shows once the
            // navigation lands even if it is still in flight.
            self.show_browser_tab(id, window, cx);
        }
        cx.notify();
        Some(Placed::Opened {
            workspace_id: workspace,
        })
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

    /// The endpoint of this window whose live snapshot lists `workspace` of
    /// `scope`, whether or not the window shows it now.
    pub(crate) fn endpoint_showing(&self, scope: &Scope, workspace: &str) -> Option<usize> {
        self.endpoints
            .iter()
            .enumerate()
            .find_map(|(index, endpoint)| {
                if &super::scope(endpoint) != scope {
                    return None;
                }
                let live = if index == self.selected_endpoint {
                    &self.live
                } else {
                    &endpoint.live
                };
                live.snapshot
                    .as_ref()?
                    .workspaces
                    .iter()
                    .any(|candidate| candidate.workspace_id == workspace)
                    .then_some(index)
            })
    }

    /// Whether the window shows `workspace` of endpoint `index` now.
    pub(crate) fn shows_workspace(&self, index: usize, workspace: &str) -> bool {
        index == self.selected_endpoint
            && self
                .live
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.focused_workspace_id.as_deref())
                == Some(workspace)
    }

    /// Switches to `workspace` of endpoint `index`, unless the window shows
    /// it already. The switch lands with a later snapshot.
    pub(crate) fn go_to_workspace(
        &mut self,
        index: usize,
        workspace: &str,
        cx: &mut Context<Self>,
    ) {
        if self.shows_workspace(index, workspace) {
            return;
        }
        if index != self.selected_endpoint {
            let endpoint = self.endpoints[index].id.clone();
            self.navigate_endpoint(&endpoint, NavigationTarget::Workspace(workspace), cx);
        } else {
            self.navigate(NavigationTarget::Workspace(workspace), cx);
        }
    }
}

/// The text a control request reads from a page: `null` until the page has
/// loaded, then its visible text, bounded before it leaves the page. The
/// app's own script; nothing a caller sends is ever run.
#[cfg(any(target_os = "macos", windows))]
const PAGE_TEXT_SCRIPT: &str = "(function () {\
  if (document.readyState !== 'complete') { return null; }\
  var body = document.body;\
  return body ? String(body.innerText).slice(0, 65536) : '';\
})()";

/// A control request's view of this window's native pages.
#[cfg(any(target_os = "macos", windows))]
impl HerdrWindow {
    /// Whether this window has made the page of tab `id`.
    pub(crate) fn has_page(&self, id: TabId) -> bool {
        self.browser.pages.contains(id)
    }

    /// Loads `location` in tab `id`'s page, if this window has made it.
    pub(crate) fn load_requested_page(&self, id: TabId, location: &Location, cx: &mut App) {
        self.browser.pages.load(id, location, cx);
    }

    /// Goes back, or forward, in tab `id`'s page. Whether the window has it.
    pub(crate) fn page_history(&self, id: TabId, forward: bool, cx: &App) -> bool {
        if !self.has_page(id) {
            return false;
        }
        if forward {
            self.browser.pages.forward(id, cx);
        } else {
            self.browser.pages.back(id, cx);
        }
        true
    }

    /// Asks tab `id`'s page for its text, handed to `done` as JSON: `null`
    /// while the page is still loading. Whether the window has the page.
    pub(crate) fn page_text(
        &self,
        id: TabId,
        done: impl Fn(String) + Send + 'static,
        cx: &App,
    ) -> crate::Result<bool> {
        let Some(page) = self.browser.pages.page(id) else {
            return Ok(false);
        };
        page.read(cx)
            .raw()
            .evaluate_script_with_callback(PAGE_TEXT_SCRIPT, done)?;
        Ok(true)
    }
}

#[cfg(target_os = "macos")]
impl HerdrWindow {
    /// Takes a picture of tab `id`'s page as it shows now, handed to `done`
    /// as TIFF bytes. Whether one was asked for: the page must be on
    /// screen, where it was laid out last frame.
    pub(crate) fn page_screenshot(
        &self,
        id: TabId,
        done: impl FnOnce(Option<Vec<u8>>) + 'static,
        cx: &App,
    ) -> bool {
        let Some(page) = self.browser.pages.page(id) else {
            return false;
        };
        if !self.browser.pages.is_shown(id) {
            return false;
        }
        let Some(bounds) = self.browser.page_bounds.borrow().get(&id).copied() else {
            return false;
        };
        let rect = crate::browser::annotate::Rect {
            x: 0.,
            y: 0.,
            width: f64::from(f32::from(bounds.size.width)),
            height: f64::from(f32::from(bounds.size.height)),
        };
        crate::browser::snapshot::capture(page.read(cx).raw(), rect, done);
        true
    }
}
