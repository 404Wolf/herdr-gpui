//! Files dropped or pasted into a WSL distribution's pane paste as that
//! distribution's own paths for them: `/mnt/c/…` for a drive, or `/home/…`
//! for a file inside it, mapped by its `wslpath` on a background worker.
//! Nothing is copied, since a distribution already reads this machine's
//! drives, so a file of any size is ready at once and nothing is left behind.

use super::{HerdrWindow, file_drop::quote_paths};
use crate::{Error, connection::ConnectionBridge, terminal::InputTarget};
use gpui::Context;
use herdr_client::protocol::ClientPaneInputEvent;
use std::{path::PathBuf, sync::atomic::AtomicBool};

impl HerdrWindow {
    pub(super) fn paste_wsl_files(
        &mut self,
        target: InputTarget,
        distro: String,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let endpoint = &self.endpoints[self.selected_endpoint];
        let Some(snapshot) = &self.live.snapshot else {
            return;
        };
        let endpoint_id = endpoint.id.clone();
        let generation = endpoint.generation;
        let epoch = self.selection_epoch;
        let boot = snapshot.boot_id.clone();
        // Mapping is instant once the distribution runs; starting a stopped
        // one is bounded by herdr-client's idle limit, so nothing cancels it.
        let work = cx.background_executor().spawn(async move {
            herdr_client::wsl_paths(&distro, &paths, &AtomicBool::new(false))
        });
        cx.spawn(async move |this, cx| {
            let mapped = work.await;
            let _ = this.update(cx, |this, cx| {
                let endpoint = &this.endpoints[this.selected_endpoint];
                // The user moved on to another device or connection meanwhile;
                // these paths name nothing there.
                if this.selection_epoch != epoch
                    || endpoint.generation != generation
                    || endpoint.id != endpoint_id
                {
                    return;
                }
                let pasted = mapped.map_err(Error::from).and_then(|mapped| {
                    let text =
                        quote_paths(&mapped.into_iter().map(PathBuf::from).collect::<Vec<_>>())?;
                    if this.menu.page.is_some() || !this.input_ready() {
                        return Err(Error::ConnectionNotReady);
                    }
                    let handle = endpoint
                        .connection
                        .handle
                        .as_ref()
                        .ok_or(Error::NotConnected)?;
                    ConnectionBridge::send_input(
                        handle,
                        &boot,
                        &target,
                        ClientPaneInputEvent::Paste(text),
                    )?;
                    Ok(())
                });
                if let Err(error) = pasted {
                    this.local_error = Some(format!("Files not pasted: {error}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
