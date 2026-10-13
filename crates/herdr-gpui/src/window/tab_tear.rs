//! Carrying a tab out of the window to open it in a new one, as a web
//! browser does. Every window lists the same tabs for a workspace, Herdr's
//! and this client's own alike, so the tab is not taken from this window:
//! the new one is another client of the same daemon that goes to the tab's
//! workspace and shows it, while the group here moves on to a neighbour.

use super::{HerdrWindow, tab_drag::TabDrag};
use crate::{
    NavigationTarget,
    app::Placement,
    browser::{Pick, Store},
    window_state::WindowState,
};
use gpui::{Bounds, Context, Window};
use herdr_client::ConnectTarget;

/// A tab carried out of a window, and where the window that shows it opens.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TornTab {
    /// The daemon the new window connects to, as New Window's does.
    pub(crate) target: ConnectTarget,
    /// The host the tab belongs to, which the new window shows.
    endpoint: String,
    pub(crate) pick: Pick,
    pub(crate) placement: Placement,
}

/// The tabs torn off in a test, which opens no windows.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TornOff(pub(crate) Vec<TornTab>);

#[cfg(test)]
impl gpui::Global for TornOff {}

impl HerdrWindow {
    /// Opens the tab `drag` carried out of the window in a new window,
    /// placed as if this one had been carried along with it.
    pub(super) fn tear_off_tab(
        &mut self,
        drag: &TabDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let frame = window.bounds();
        let bounds = Bounds::new(frame.origin + (drag.pointer - drag.origin), frame.size);
        let torn = TornTab {
            target: self.endpoints[0].connection.target.clone(),
            endpoint: self.endpoints[self.selected_endpoint].id.clone(),
            pick: drag.pick.clone(),
            placement: WindowState::beside(bounds, window, cx),
        };
        self.leave_tab(&drag.pick, cx);
        #[cfg(test)]
        cx.default_global::<TornOff>().0.push(torn);
        #[cfg(not(test))]
        crate::app::open_additional_window_then(
            torn.target.clone(),
            Some(torn.placement),
            move |view, window, cx| view.arrive(&torn, window, cx),
            cx,
        );
    }

    /// Goes to the tab `torn` names, in a window just opened for it. The
    /// window is not connected yet, so the navigation waits for its
    /// surface, and a page is chosen against its workspace to show once
    /// the navigation lands.
    pub(crate) fn arrive(&mut self, torn: &TornTab, window: &mut Window, cx: &mut Context<Self>) {
        match &torn.pick {
            Pick::Herdr(tab) => {
                self.navigate_endpoint(&torn.endpoint, NavigationTarget::Tab(tab), cx);
            }
            Pick::Page(id) => {
                let Some(workspace) = cx
                    .try_global::<Store>()
                    .and_then(|store| store.get(*id))
                    .map(|tab| tab.workspace_id.clone())
                else {
                    return;
                };
                self.show_browser_tab(*id, window, cx);
                self.navigate_endpoint(&torn.endpoint, NavigationTarget::Workspace(&workspace), cx);
            }
        }
    }
}
