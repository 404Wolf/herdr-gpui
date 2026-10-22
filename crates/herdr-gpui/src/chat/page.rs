//! A web page the user previews from a chat: an HTML fence the agent
//! wrote, or an HTML file one of its tool calls wrote. Where pages show in
//! the window (macOS and Windows), it opens in a panel beside the entries,
//! served from its folder as a local browser tab is; elsewhere it opens in
//! the system browser. A fence is written to a file of its own first, in a
//! folder only this user can read, so the page reads nothing else.
//!
//! The page is the agent's, so it is untrusted: it only ever loads because
//! the user asked, and it gets what a local browser tab gets, no more.

use crate::{HerdrWindow, browser::TabId, notifications::safe_text};
use gpui::{AppContext as _, Context, Window};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    rc::Rc,
};

/// Narrowest pane that shows the page beside the entries.
pub(crate) const MIN_PANE: f32 = 640.;

/// The page a chat's panel shows.
#[derive(Debug)]
pub(crate) struct ChatPage {
    pub(crate) id: TabId,
    pub(crate) title: String,
}

/// What to preview.
#[derive(Clone, Debug)]
pub(crate) enum PageSource {
    /// An HTML fence's source.
    Html(Rc<str>),
    /// An HTML file on the chat's machine.
    File(PathBuf),
}

impl HerdrWindow {
    /// Shows `source` beside the chat in `pane_id`, or in the system
    /// browser where pages cannot show in the window.
    pub(crate) fn preview_chat_page(
        &mut self,
        pane_id: &str,
        source: PageSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.chats.pane_mut(pane_id) else {
            return;
        };
        match source {
            PageSource::File(path) => {
                if let super::Route::Ssh(host) = &pane.target.route {
                    pane.error = Some(format!(
                        "That page is on {}; open it there to see it",
                        safe_text(host, 80)
                    ));
                    cx.notify();
                    return;
                }
                let title = path.file_name().map_or_else(
                    || "Page".to_owned(),
                    |name| name.to_string_lossy().into_owned(),
                );
                self.show_chat_file(pane_id, path, title, window, cx);
            }
            PageSource::Html(html) => {
                let html = html.to_string();
                let written = cx.background_spawn(async move { write_page(&html) });
                let pane_id = pane_id.to_owned();
                cx.spawn_in(window, async move |this, cx| {
                    let written = written.await;
                    let _ = this.update_in(cx, |this, window, cx| match written {
                        Ok(path) => {
                            this.show_chat_file(&pane_id, path, "HTML page".to_owned(), window, cx);
                        }
                        Err(error) => {
                            if let Some(pane) = this.chats.pane_mut(&pane_id) {
                                pane.error = Some(format!("The page could not be shown: {error}"));
                            }
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        }
    }

    #[cfg(any(target_os = "macos", windows))]
    fn show_chat_file(
        &mut self,
        pane_id: &str,
        path: PathBuf,
        title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let home = crate::config::home().ok();
        let file = match crate::browser::LocalFile::new(&path, home.as_deref()) {
            Ok(file) => file,
            Err(error) => {
                if let Some(pane) = self.chats.pane_mut(pane_id) {
                    pane.error = Some(format!("The page could not be shown: {error}"));
                }
                cx.notify();
                return;
            }
        };
        self.close_chat_panel(pane_id, cx);
        self.chats.opened_pages += 1;
        let id = TabId::panel(self.chats.opened_pages);
        if let Err(error) = self.open_chat_page(id, file, title.clone(), window, cx) {
            if let Some(pane) = self.chats.pane_mut(pane_id) {
                pane.error = Some(format!("The page could not be shown: {error}"));
            }
            cx.notify();
            return;
        }
        if let Some(pane) = self.chats.pane_mut(pane_id) {
            pane.page = Some(ChatPage { id, title });
        } else {
            self.close_chat_page(id);
        }
        cx.notify();
    }

    /// Without pages in the window, the system browser shows the file.
    #[cfg(not(any(target_os = "macos", windows)))]
    fn show_chat_file(
        &mut self,
        pane_id: &str,
        path: PathBuf,
        _title: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match url::Url::from_file_path(&path) {
            Ok(url) => {
                cx.open_url(url.as_str());
                self.show_flash(
                    crate::window::Flash::success("Opened the page in your browser"),
                    cx,
                );
            }
            Err(()) => {
                if let Some(pane) = self.chats.pane_mut(pane_id) {
                    pane.error = Some("The page could not be shown: not a file path".to_owned());
                }
                cx.notify();
            }
        }
    }

    /// Closes the page beside the chat in `pane_id`.
    pub(crate) fn close_chat_panel(&mut self, pane_id: &str, cx: &mut Context<Self>) {
        let Some(page) = self
            .chats
            .pane_mut(pane_id)
            .and_then(|pane| pane.page.take())
        else {
            return;
        };
        #[cfg(any(target_os = "macos", windows))]
        self.close_chat_page(page.id);
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = page;
        cx.notify();
    }

    /// The panel pages the window draws this frame: those of chats on
    /// screen in panes wide enough for the panel.
    pub(crate) fn shown_chat_pages(&self) -> Vec<TabId> {
        let Some(surface) = self
            .live
            .surface
            .as_ref()
            .filter(|surface| surface.popup.is_none())
        else {
            return Vec::new();
        };
        surface
            .panes
            .iter()
            .filter(|pane| {
                f32::from(pane.inner_rect.width) * self.cell_width >= MIN_PANE
                    && !self.chats.raw(&pane.pane_id)
            })
            .filter_map(|pane| self.chats.pane(&pane.pane_id))
            .filter(|chat| chat.available())
            .filter_map(|chat| chat.page.as_ref().map(|page| page.id))
            .collect()
    }
}

/// Writes an HTML fence to `index.html` in a folder of its own, named by
/// what it holds, and returns the file. Blocks, so it runs off the UI
/// thread.
fn write_page(html: &str) -> std::io::Result<PathBuf> {
    let root = pages_root().ok_or_else(|| std::io::Error::other("no folder for pages"))?;
    let mut hasher = DefaultHasher::new();
    html.hash(&mut hasher);
    let folder = root.join(format!("{:016x}", hasher.finish()));
    private_dir(&root)?;
    private_dir(&folder)?;
    let file = folder.join("index.html");
    std::fs::write(&file, html)?;
    Ok(file)
}

/// The per-user folder pages are written in. A local page may not be
/// served from a folder with a hidden name in its path, so on macOS and
/// Windows it is the user's own temporary folder.
#[cfg(any(target_os = "macos", windows))]
fn pages_root() -> Option<PathBuf> {
    Some(std::env::temp_dir().join("herdr-chat-pages"))
}

/// Elsewhere `/tmp` is shared with other users, so the page goes in the
/// user's runtime or cache folder.
#[cfg(not(any(target_os = "macos", windows)))]
fn pages_root() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    base.is_absolute()
        .then(|| base.join("herdr-gpui-chat-pages"))
}

/// Creates `path`, readable by this user alone where the platform says so.
fn private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
}

#[cfg(test)]
mod tests;
