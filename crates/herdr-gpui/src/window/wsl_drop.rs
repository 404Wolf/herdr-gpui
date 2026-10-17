//! Files dropped or pasted into a WSL distribution's pane paste as that
//! distribution's own paths for them: `/mnt/c/…` for a drive, or `/home/…`
//! for a file inside it, mapped by its `wslpath` on a background worker.
//! Nothing is copied, since a distribution already reads this machine's
//! drives, so a file of any size is ready at once and nothing is left behind.
//!
//! Mapping takes a `wsl.exe` round trip, so keys typed meanwhile are held
//! (see `pending_input`) and sent after the paths, as they were typed. One
//! drop maps at a time.

use super::{HerdrWindow, file_drop::quote_paths};
use crate::{Error, Result, connection::ConnectionBridge, terminal::InputTarget};
use gpui::Context;
use herdr_client::protocol::ClientPaneInputEvent;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

/// A drop whose paths are being mapped, and where they were meant to go.
pub(crate) struct WslPaste {
    /// Tells this drop's answer from a later drop's once a reconnect has
    /// forgotten this one.
    token: Arc<()>,
    endpoint: String,
    generation: u64,
    epoch: u64,
    boot: String,
    target: InputTarget,
}

impl HerdrWindow {
    pub(super) fn paste_wsl_files(
        &mut self,
        target: InputTarget,
        distro: String,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if self.wsl_paste.is_some() {
            return Err(Error::WslPasteBusy);
        }
        // Records where held keys go before any are held.
        self.flush_pending_input(cx);
        let endpoint = &self.endpoints[self.selected_endpoint];
        let snapshot = self.live.snapshot.as_ref().ok_or(Error::NoSnapshot)?;
        let token = Arc::new(());
        let answer = token.clone();
        self.wsl_paste = Some(WslPaste {
            token,
            endpoint: endpoint.id.clone(),
            generation: endpoint.generation,
            epoch: self.selection_epoch,
            boot: snapshot.boot_id.clone(),
            target,
        });
        // Mapping is instant once the distribution runs; starting a stopped
        // one is bounded by herdr-client's idle limit, so nothing cancels it.
        let work = cx.background_executor().spawn(async move {
            herdr_client::wsl_paths(&distro, &paths, &AtomicBool::new(false))
        });
        cx.spawn(async move |this, cx| {
            let mapped = work.await;
            let _ = this.update(cx, |this, cx| this.finish_wsl_paste(&answer, mapped, cx));
        })
        .detach();
        Ok(())
    }

    fn finish_wsl_paste(
        &mut self,
        answer: &Arc<()>,
        mapped: herdr_client::Result<Vec<String>>,
        cx: &mut Context<Self>,
    ) {
        // A reconnect or device switch already forgot this drop.
        if !self
            .wsl_paste
            .as_ref()
            .is_some_and(|paste| Arc::ptr_eq(&paste.token, answer))
        {
            return;
        }
        let Some(paste) = self.wsl_paste.take() else {
            return;
        };
        let pasted = mapped
            .map_err(Error::from)
            .and_then(|mapped| self.send_wsl_paths(&paste, mapped));
        if let Err(error) = pasted {
            self.local_error = Some(format!("Files not pasted: {error}"));
            cx.notify();
        }
        // Keys typed while the paths were mapped follow them, or are refused
        // with a reason if focus moved meanwhile.
        self.flush_pending_input(cx);
    }

    fn send_wsl_paths(&self, paste: &WslPaste, mapped: Vec<String>) -> Result<()> {
        let endpoint = &self.endpoints[self.selected_endpoint];
        // The window moved on to another device or connection, or the pane
        // or popup the files were dropped on no longer takes input.
        let current = paste.epoch == self.selection_epoch
            && paste.generation == endpoint.generation
            && paste.endpoint == endpoint.id
            && self.menu.page.is_none()
            && self.input_ready()
            && self
                .live
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.boot_id == paste.boot)
            && self.input_target_shown(&paste.target);
        if !current {
            return Err(Error::WslPasteTargetGone);
        }
        let text = quote_paths(&mapped.into_iter().map(PathBuf::from).collect::<Vec<_>>())?;
        let handle = endpoint
            .connection
            .handle
            .as_ref()
            .ok_or(Error::NotConnected)?;
        ConnectionBridge::send_input(
            handle,
            &paste.boot,
            &paste.target,
            ClientPaneInputEvent::Paste(text),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
