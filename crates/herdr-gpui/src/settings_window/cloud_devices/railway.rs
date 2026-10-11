//! The Railway tab of Settings > Cloud Devices. Railway's free VM needs no
//! account, so the tab is about the SSH identity: choose a key the user
//! already has or let Herdr generate one, then connect, install Herdr there
//! (after approval), and save the device with that key attached. Setup runs as
//! a job in the main window, which shows progress in the device picker; reads
//! and removals run through `cloud::worker`, never on the UI thread.

use super::super::{Section, SettingsWindow};
use super::input;
use crate::{
    cloud::{
        CloudProvider,
        worker::{self, Worker},
    },
    config::corners,
    railway::setup::{self, AddRequest, Choice, Overview},
    search_input::SearchInput,
};
use gpui::{prelude::*, *};
use std::path::{Path, PathBuf};

/// Where the key for a new device comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeySource {
    Generate,
    Existing,
}

enum Update {
    Overview(crate::railway::Result<Overview>),
    Done(crate::railway::Result<()>),
}

enum Account {
    Checking,
    Known(Overview),
}

pub(in crate::settings_window) struct RailwayCard {
    path: Entity<SearchInput>,
    source: KeySource,
    /// The user's approval to install Herdr on VMs set up from here.
    install: bool,
    account: Account,
    message: Option<String>,
    job: Option<Worker>,
    /// Tests build the card without the user's real saved devices.
    #[cfg(test)]
    reads_disk: bool,
}

impl RailwayCard {
    fn new(
        config: &crate::config::Config,
        theme: &crate::config::Theme,
        cx: &mut Context<SettingsWindow>,
    ) -> Self {
        Self {
            path: input("/Users/you/.ssh/id_ed25519", "", config, theme, cx),
            source: KeySource::Generate,
            install: true,
            account: Account::Checking,
            message: None,
            job: None,
            #[cfg(test)]
            reads_disk: false,
        }
    }

    fn devices(&self) -> &[setup::Device] {
        match &self.account {
            Account::Known(overview) => &overview.devices,
            Account::Checking => &[],
        }
    }
}

/// What a device row says about its key and the VM's window.
fn describe(device: &setup::Device, now: u64) -> String {
    let key = match device.record.as_ref().map(|record| &record.source) {
        None => "no saved key".to_owned(),
        Some(crate::railway::Source::Generated) => "generated key".to_owned(),
        Some(crate::railway::Source::Borrowed { path }) => format!("key {path}"),
    };
    let state = match &device.record {
        Some(_) if !device.key_present => " · key file missing",
        Some(record) if record.claimed => " · claimed",
        Some(record) if record.window_ended(now) => " · build window probably ended",
        _ => "",
    };
    format!("{} · {key}{state}", device.saved.machine)
}

impl SettingsWindow {
    pub(super) fn open_railway_card(&mut self, cx: &mut Context<Self>) {
        if self.railway_card.is_none() {
            self.railway_card = Some(RailwayCard::new(&self.config, &self.theme, cx));
        }
        self.refresh_railway(cx);
    }

    fn railway_job(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce(&dyn Fn() -> bool, &dyn Fn(Update)) + Send + 'static,
    ) {
        let Some(card) = &mut self.railway_card else {
            return;
        };
        card.job = None;
        match worker::spawn(
            "herdr-railway-settings",
            cx,
            work,
            |this: &mut Self, update, cx| this.apply_railway(update, cx),
        ) {
            Ok(job) => {
                if let Some(card) = &mut self.railway_card {
                    card.job = Some(job);
                }
            }
            Err(error) => {
                tracing::error!(category = "railway_worker", error_kind = ?error.kind(), "Could not start Railway settings worker");
                if let Some(card) = &mut self.railway_card {
                    card.message = Some(crate::cloud::Error::Worker("settings").to_string());
                }
            }
        }
    }

    fn refresh_railway(&mut self, cx: &mut Context<Self>) {
        #[cfg(test)]
        if self
            .railway_card
            .as_ref()
            .is_some_and(|card| !card.reads_disk)
        {
            return;
        }
        if let Some(card) = &mut self.railway_card {
            card.account = Account::Checking;
        }
        self.railway_job(cx, |_, send| send(Update::Overview(setup::overview())));
    }

    fn apply_railway(&mut self, update: Update, cx: &mut Context<Self>) {
        let Some(card) = &mut self.railway_card else {
            return;
        };
        match update {
            Update::Overview(Ok(overview)) => card.account = Account::Known(overview),
            Update::Overview(Err(error)) => {
                card.account = Account::Known(Overview {
                    devices: Vec::new(),
                    pending_key: None,
                    detected: Vec::new(),
                });
                card.message = Some(error.to_string());
            }
            Update::Done(result) => {
                card.message = result.err().map(|error| error.to_string());
                self.refresh_railway(cx);
            }
        }
    }

    pub(super) fn railway_config_changed(&mut self, cx: &mut Context<Self>) {
        if self.railway_card.is_some() && self.section == Section::CloudDevices {
            self.refresh_railway(cx);
        }
    }

    fn forget_railway_device(&mut self, id: String, cx: &mut Context<Self>) {
        self.railway_job(cx, move |_, send| {
            send(Update::Done(setup::forget_device(&id)));
        });
    }

    fn generate_railway_key(&mut self, cx: &mut Context<Self>) {
        self.railway_job(cx, |cancelled, send| {
            send(Update::Done(setup::generate_key(cancelled).map(drop)));
        });
    }

    fn claim_railway_device(&mut self, id: String, cx: &mut Context<Self>) {
        self.railway_job(cx, move |_, send| {
            send(Update::Done(setup::mark_claimed(&id)));
        });
    }

    fn use_railway_key(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(card) = &mut self.railway_card {
            card.source = KeySource::Existing;
            card.path.update(cx, |input, cx| {
                input.set_text_selected(&path.to_string_lossy(), cx)
            });
        }
        cx.notify();
    }

    /// The system file dialog; the chosen path fills the field, and nothing is
    /// read from it until setup runs.
    fn browse_railway_key(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose SSH private key".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                let _ = this.update(cx, |this, cx| this.use_railway_key(&path, cx));
            }
        })
        .detach();
    }

    /// Hand a setup to the main window, which runs and reports the job so it
    /// survives this window closing. The choice was made explicitly here;
    /// nothing else starts one.
    fn start_railway_setup(&mut self, choice: Choice, name: String, cx: &mut Context<Self>) {
        let Some(card) = &mut self.railway_card else {
            return;
        };
        let request = AddRequest {
            choice,
            install: card.install,
        };
        let started = self.source.update(cx, |window, cx| {
            if window.cloud_jobs.has(CloudProvider::Railway) {
                return Ok(Err(crate::cloud::Error::Unavailable(
                    "A Railway setup is already running.",
                )
                .into()));
            }
            window
                .start_cloud_job(
                    CloudProvider::Railway,
                    name.clone(),
                    move |cancelled, report| setup::add_device(request, cancelled, report),
                    cx,
                )
                .map(Ok)
        });
        if let Some(card) = &mut self.railway_card {
            card.message = Some(match started {
                Ok(Ok(Ok(()))) => {
                    format!("Connecting {name}. The device picker shows its progress.")
                }
                Ok(Ok(Err(error)) | Err(error)) => error.to_string(),
                Err(_) => "Open a main Herdr window to set up a VM.".into(),
            });
        }
        cx.notify();
    }

    fn setup_railway_new(&mut self, cx: &mut Context<Self>) {
        let Some(card) = &mut self.railway_card else {
            return;
        };
        let choice = match card.source {
            KeySource::Generate => Choice::Generate,
            KeySource::Existing => {
                let text = card.path.read(cx).text().trim().to_owned();
                if text.is_empty() {
                    card.message = Some("Enter or choose the path of an SSH private key.".into());
                    cx.notify();
                    return;
                }
                Choice::Existing(setup::expand(&text))
            }
        };
        self.start_railway_setup(choice, setup::random_name(), cx);
    }

    fn reconnect_railway_device(&mut self, id: String, label: String, cx: &mut Context<Self>) {
        self.start_railway_setup(Choice::Saved(id), label, cx);
    }

    fn railway_button(
        &self,
        id: &'static str,
        label: &'static str,
        enabled: bool,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.cloud_button(id, label, enabled)
            .debug_selector(move || id.to_owned())
            .when(enabled, |button| {
                button.on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
            })
    }

    fn railway_device_row(
        &self,
        index: usize,
        device: &setup::Device,
        ready: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let (id, label) = (device.saved.id.clone(), device.saved.label.clone());
        let claim = device.record.as_ref().is_some_and(|record| !record.claimed);
        let reconnect = ready && device.record.is_some() && crate::cloud::unavailable().is_none();
        let button = |name: &'static str, text: &'static str, enabled: bool| {
            self.cloud_button(
                SharedString::from(format!("cloud-railway-{name}-{index}")),
                text,
                enabled,
            )
            .debug_selector(move || format!("cloud-railway-{name}-{index}"))
        };
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(
                div()
                    .min_w_0()
                    .child(div().truncate().child(device.saved.label.clone()))
                    .child(
                        div()
                            .truncate()
                            .text_color(rgb(self.theme.muted))
                            .child(describe(device, crate::railway::now())),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        button("reconnect", "Reconnect", reconnect).when(reconnect, |b| {
                            let (id, label) = (id.clone(), label.clone());
                            b.on_click(cx.listener(move |this, _, _, cx| {
                                this.reconnect_railway_device(id.clone(), label.clone(), cx)
                            }))
                        }),
                    )
                    .when(claim, |row| {
                        let id = id.clone();
                        row.child(button("claimed", "Mark claimed", ready).when(ready, |b| {
                            b.on_click(cx.listener(move |this, _, _, cx| {
                                this.claim_railway_device(id.clone(), cx)
                            }))
                        }))
                    })
                    .child(button("remove", "Remove", ready).when(ready, |b| {
                        let id = id.clone();
                        b.on_click(cx.listener(move |this, _, _, cx| {
                            this.forget_railway_device(id.clone(), cx)
                        }))
                    })),
            )
    }

    pub(super) fn render_railway_card(&self, cx: &mut Context<Self>) -> Div {
        let Some(card) = &self.railway_card else {
            return div();
        };
        let ready = !self.busy();
        let available = ready && crate::cloud::unavailable().is_none();
        let (pending, detected): (Option<&Path>, &[PathBuf]) = match &card.account {
            Account::Known(overview) => (overview.pending_key.as_deref(), &overview.detected),
            Account::Checking => (None, &[]),
        };
        let mut about = self
            .control_card("Railway")
            .debug_selector(|| "cloud-railway-card".into())
            .child(self.control_note(
                "Railway gives anyone a free Linux VM over SSH, with no account: connecting to railway.new creates one for the SSH key you present, and the same key always reaches the same VM.",
            ))
            .child(self.control_note(
                "Railway's limits: 2 vCPU and 2 GB, 60 minutes to build and then 24 hours to claim it before it is deleted, 3 new VMs per IP address per day, IPv4 only.",
            ))
            .child(self.control_note(
                "Herdr connects only with the key saved on each device and remembers railway.new's SSH host key the first time. Reconnecting never creates a key or installs anything. After the build window Herdr stops and asks first, because connecting again could create a different, empty VM.",
            ));
        if let Some(reason) = crate::cloud::unavailable() {
            about = about.child(self.control_note(reason));
        }
        let mut key = self
            .control_card("SSH key")
            .debug_selector(|| "cloud-railway-key".into())
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(self.railway_choice(
                        "cloud-railway-source-generate",
                        "Generate a new key",
                        KeySource::Generate,
                        card.source,
                        ready,
                        cx,
                    ))
                    .child(self.railway_choice(
                        "cloud-railway-source-existing",
                        "Use an existing key",
                        KeySource::Existing,
                        card.source,
                        ready,
                        cx,
                    )),
            );
        key = match card.source {
            KeySource::Generate => key.child(self.control_note(if pending.is_some() {
                "Herdr generated this key and keeps it until a device uses it, so a retry reaches the same VM. Removing a device deletes its generated key."
            } else {
                "Herdr creates an ed25519 key without a passphrase in its private state folder, saves it with the device, and uses it for every reconnect. Removing the device deletes it."
            })),
            KeySource::Existing => {
                let mut existing = key
                    .child(self.cloud_field("Private key path", card.path.clone().into()))
                    .child(self.control_note(
                        "Herdr passes this path to ssh and reads only the key's fingerprint. The file is never copied, changed, or deleted. A passphrase-protected key needs its .pub file beside it and must be loaded in your SSH agent.",
                    ));
                let mut row = div().flex().flex_wrap().gap(px(8.)).child(self.railway_button(
                    "cloud-railway-browse",
                    "Browse…",
                    ready,
                    |this, cx| this.browse_railway_key(cx),
                    cx,
                ));
                for (index, path) in detected.iter().enumerate() {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let path = path.clone();
                    row = row.child(
                        self.cloud_button(
                            SharedString::from(format!("cloud-railway-detected-{index}")),
                            format!("~/.ssh/{name}"),
                            ready,
                        )
                        .debug_selector(move || format!("cloud-railway-detected-{index}"))
                        .when(ready, |button| {
                            button.on_click(
                                cx.listener(move |this, _, _, cx| this.use_railway_key(&path, cx)),
                            )
                        }),
                    );
                }
                existing = existing.child(row);
                existing
            }
        };
        // The first connection for a key must be made by the user.
        let first_use = match card.source {
            KeySource::Generate => pending.map(Path::to_path_buf),
            KeySource::Existing => {
                let text = card.path.read(cx).text().trim().to_owned();
                (!text.is_empty()).then(|| setup::expand(&text))
            }
        };
        key = key.child(self.control_note(
            "A new key needs one interactive connection first: Railway refuses to create a VM for a script, so Herdr cannot do this step for you. Run the command below once in your own terminal, then type exit. Skip this if the key already has a VM.",
        ));
        key = match first_use {
            Some(path) => {
                let command = setup::first_use_command(&path);
                key.child(
                    div()
                        .debug_selector(|| "cloud-railway-first-use".into())
                        .min_w_0()
                        .p(px(10.))
                        .rounded(px(corners::CONTROL))
                        .border_1()
                        .border_color(rgb(self.theme.active))
                        .bg(rgb(self.theme.background))
                        .child(command.clone()),
                )
                .child(div().flex().justify_end().child(self.railway_button(
                    "cloud-railway-copy",
                    "Copy command",
                    true,
                    move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(command.clone())),
                    cx,
                )))
            }
            None if card.source == KeySource::Generate => {
                key.child(div().flex().justify_end().child(self.railway_button(
                    "cloud-railway-generate",
                    "Generate key",
                    available,
                    |this, cx| this.generate_railway_key(cx),
                    cx,
                )))
            }
            None => key,
        };
        key = key
            .child(
                self.control_switch(
                    "cloud-railway-install",
                    "Install Herdr on the VM when missing",
                    card.install,
                    ready,
                )
                .when(ready, |row| {
                    row.on_click(cx.listener(|this, _, _, cx| {
                        if let Some(card) = &mut this.railway_card {
                            card.install = !card.install;
                        }
                        cx.notify();
                    }))
                }),
            )
            .child(self.control_note(
                "Connect and set up reaches the existing VM without a terminal, installs Herdr there when missing, and saves the device. It never creates a VM.",
            ))
            .child(
                div().flex().justify_end().child(self.railway_button(
                    "cloud-railway-create",
                    "Connect and set up",
                    available,
                    |this, cx| this.setup_railway_new(cx),
                    cx,
                )),
            );
        if let Some(message) = &card.message {
            key = key.child(div().min_w_0().child(message.clone()));
        }
        let mut devices = self
            .control_card("Railway devices")
            .debug_selector(|| "cloud-railway-devices".into());
        if card.devices().is_empty() {
            devices = devices.child(self.control_note("None yet."));
        }
        for (index, device) in card.devices().iter().enumerate() {
            devices = devices.child(self.railway_device_row(index, device, ready, cx));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .min_w_0()
            .child(about)
            .child(key)
            .child(devices)
    }

    fn railway_choice(
        &self,
        id: &'static str,
        label: &'static str,
        source: KeySource,
        current: KeySource,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.control_choice(id, label, current == source, enabled)
            .debug_selector(move || id.to_owned())
            .when(enabled, |choice| {
                choice.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(card) = &mut this.railway_card {
                        card.source = source;
                    }
                    cx.notify();
                }))
            })
    }
}

#[cfg(test)]
mod tests;
