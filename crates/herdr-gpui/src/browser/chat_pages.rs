//! The page a chat shows in its panel: a local file, served from its folder
//! as a browser tab's local page is, but owned by the chat rather than the
//! store. It has no tab, so it is never saved, listed, or put in a group;
//! the chat creates it from a click, the window presents it while the chat
//! draws it, and the chat closes it when the panel or the chat goes.

use super::{LocalFile, Location, Tab, TabId, Zoom, page::PageElement, store::Place};
use crate::HerdrWindow;
use gpui::{Context, Window};

impl HerdrWindow {
    /// Creates page `id` showing `file`; one this window has already does
    /// not change. Runs from input, as creating a page starts the
    /// platform's web content processes.
    pub(crate) fn open_chat_page(
        &mut self,
        id: TabId,
        file: LocalFile,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> crate::Result<()> {
        let tab = Tab {
            id,
            scope: super::scope(&self.endpoints[self.selected_endpoint]),
            workspace_id: "chat".to_owned(),
            location: Some(Location::Local { file }),
            title,
            origin: None,
            place: Place::Group,
            zoom: Zoom::default(),
        };
        self.browser.pages.ensure(&tab, window, cx)
    }

    /// The element page `id` lays out in, once the window has the page; on
    /// Windows it arrives a moment after it is opened.
    pub(crate) fn chat_page_element(&self, id: TabId) -> Option<PageElement> {
        self.browser.pages.element(id)
    }

    pub(crate) fn close_chat_page(&mut self, id: TabId) {
        self.browser.pages.close(id);
    }
}
