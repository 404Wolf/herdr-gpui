//! Live sidebar spacing, selection and host colours: previews reach every main
//! window at once, and commits write through Settings' serial save path.
use super::{SettingsWindow, ui};
use crate::{
    config::{Config, SidebarStyle, preferences::Preference},
    search_input::SearchInput,
};
use gpui::{prelude::*, *};

/// What the Sidebar page shows and every main window uses until it is saved.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SidebarIntent {
    pub style: SidebarStyle,
}

impl SidebarIntent {
    pub(super) fn of(config: &Config) -> Self {
        Self {
            style: config.sidebar_style.clone(),
        }
    }

    pub(super) fn apply(&self, config: &mut Config) {
        config.sidebar_style = self.style.clone();
    }
}

/// The host colour being typed as hex, by host row index.
pub(super) struct HexEdit {
    pub index: usize,
    pub input: Entity<SearchInput>,
    pub invalid: bool,
    pub _blur: Subscription,
}

#[derive(Default)]
struct SidebarDraft {
    active: bool,
    revision: u64,
    intent: Option<SidebarIntent>,
}
impl Global for SidebarDraft {}

pub(crate) fn sidebar_load_revision(cx: &App) -> u64 {
    cx.try_global::<SidebarDraft>()
        .map_or(0, |draft| draft.revision)
}

pub(crate) fn apply_loaded_sidebar(config: &mut Config, revision: u64, cx: &App) {
    if let Some(draft) = cx.try_global::<SidebarDraft>()
        && (draft.active || draft.revision != revision)
        && let Some(intent) = &draft.intent
    {
        // Reads started before a commit must not put the old spacing back.
        intent.apply(config);
    }
}

fn clear_sidebar_draft(cx: &mut App) {
    let draft = cx.default_global::<SidebarDraft>();
    draft.active = false;
    draft.revision = draft.revision.wrapping_add(1);
}

/// Every key `intent` changes relative to `saved`, in one save's order.
fn diff_preferences(saved: &Config, intent: &SidebarIntent) -> Vec<Preference> {
    let (old, new) = (&saved.sidebar_style.overrides, &intent.style.overrides);
    let mut edits = Vec::new();
    for (before, after, edit) in [
        (
            old.indent,
            new.indent,
            Preference::SidebarIndent as fn(_) -> _,
        ),
        (
            old.row_padding,
            new.row_padding,
            Preference::SidebarRowPadding,
        ),
        (old.gap, new.gap, Preference::SidebarRowGap),
        (old.host_gap, new.host_gap, Preference::SidebarHostGap),
    ] {
        if before != after {
            edits.push(edit(after));
        }
    }
    if saved.sidebar_style.select != intent.style.select {
        edits.push(Preference::SidebarSelect(intent.style.select));
    }
    if saved.sidebar_style.device_footer != intent.style.device_footer {
        edits.push(Preference::SidebarDeviceFooter(intent.style.device_footer));
    }
    let (old, new) = (&saved.sidebar_style.hosts, &intent.style.hosts);
    edits.extend(
        old.keys()
            .filter(|host| !new.contains_key(*host))
            .map(|host| Preference::HostColor {
                host: host.clone(),
                color: None,
            }),
    );
    edits.extend(
        new.iter()
            .filter(|(host, color)| old.get(*host) != Some(color))
            .map(|(host, color)| Preference::HostColor {
                host: host.clone(),
                color: Some(*color),
            }),
    );
    edits
}

/// Every key of `intent`, for a quit that cannot know what an in-flight save
/// left on disk. Hosts any earlier version had are cleared if `intent` lacks
/// them.
fn all_preferences<'a>(
    intent: &SidebarIntent,
    earlier: impl IntoIterator<Item = &'a SidebarStyle>,
) -> Vec<Preference> {
    let overrides = &intent.style.overrides;
    let mut edits = vec![
        Preference::SidebarIndent(overrides.indent),
        Preference::SidebarRowPadding(overrides.row_padding),
        Preference::SidebarRowGap(overrides.gap),
        Preference::SidebarHostGap(overrides.host_gap),
        Preference::SidebarSelect(intent.style.select),
        Preference::SidebarDeviceFooter(intent.style.device_footer),
    ];
    let mut removed: Vec<&String> = earlier
        .into_iter()
        .flat_map(|style| style.hosts.keys())
        .filter(|host| !intent.style.hosts.contains_key(*host))
        .collect();
    removed.sort();
    removed.dedup();
    edits.extend(removed.into_iter().map(|host| Preference::HostColor {
        host: host.clone(),
        color: None,
    }));
    edits.extend(
        intent
            .style
            .hosts
            .iter()
            .map(|(host, color)| Preference::HostColor {
                host: host.clone(),
                color: Some(*color),
            }),
    );
    edits
}

impl SettingsWindow {
    pub(super) fn sidebar_dirty(&self) -> bool {
        self.sidebar_intent.is_some()
    }

    /// Live, unsaved: updates the draft global and every HerdrWindow.
    pub(super) fn preview_sidebar(&mut self, intent: SidebarIntent, cx: &mut Context<Self>) {
        if self.quitting || self.closing.is_some() {
            return;
        }
        intent.apply(&mut self.config);
        let draft = cx.default_global::<SidebarDraft>();
        draft.active = true;
        draft.intent = Some(intent.clone());
        draft.revision = draft.revision.wrapping_add(1);
        self.sidebar_intent = Some(intent);
        self.broadcast_sidebar(cx);
    }

    /// Drops the live draft unwritten and shows the loaded values again.
    #[cfg(all(feature = "integration-test", feature = "mockup", target_os = "macos"))]
    pub(super) fn discard_sidebar_draft(&mut self, cx: &mut Context<Self>) {
        self.sidebar_intent = None;
        self.sidebar_queued = false;
        clear_sidebar_draft(cx);
        cx.default_global::<SidebarDraft>().intent = None;
        SidebarIntent::of(&self.loaded_config).apply(&mut self.config);
        self.broadcast_sidebar(cx);
    }

    /// Live and queued for save: preview, then write every changed key
    /// through one serial save.
    pub(super) fn commit_sidebar(&mut self, intent: SidebarIntent, cx: &mut Context<Self>) {
        if self.quitting || self.closing.is_some() {
            return;
        }
        self.preview_sidebar(intent, cx);
        self.sidebar_queued = true;
        self.save_sidebar_draft(cx);
    }

    pub(super) fn broadcast_sidebar(&self, cx: &mut Context<Self>) {
        let intent = SidebarIntent::of(&self.config);
        for handle in cx.windows() {
            if let Some(handle) = handle.downcast::<crate::HerdrWindow>() {
                let _ = handle.update(cx, |view, _, cx| {
                    intent.apply(&mut view.config);
                    cx.notify();
                });
            }
        }
        // Windows opened later start from the draft too.
        let mut appearance = cx
            .try_global::<crate::app::InitialAppearance>()
            .cloned()
            .unwrap_or_default();
        intent.apply(&mut appearance.config);
        cx.set_global(appearance);
        cx.notify();
    }

    fn sidebar_operation(
        &self,
        intent: &SidebarIntent,
    ) -> impl FnOnce() -> crate::Result<()> + Send + 'static + use<> {
        self.sidebar_write(diff_preferences(&self.loaded_config, intent))
    }

    fn sidebar_write(
        &self,
        edits: Vec<Preference>,
    ) -> impl FnOnce() -> crate::Result<()> + Send + 'static + use<> {
        #[cfg(test)]
        let io = self.controls.preference_io.clone();
        move || {
            #[cfg(test)]
            if let Some(io) = io {
                return edits.into_iter().try_for_each(|edit| (io.write)(edit));
            }
            edits.into_iter().try_for_each(Config::save_preference)
        }
    }

    /// Writes the draft unless another save is running; that save's
    /// completion resumes a queued draft through `finish_close`.
    pub(super) fn save_sidebar_draft(&mut self, cx: &mut Context<Self>) {
        let Some(intent) = self.sidebar_intent.clone() else {
            self.sidebar_queued = false;
            return;
        };
        if self.busy() {
            return;
        }
        self.sidebar_queued = false;
        if diff_preferences(&self.loaded_config, &intent).is_empty() {
            // Back where the file already is: nothing to write.
            self.sidebar_intent = None;
            clear_sidebar_draft(cx);
            self.finish_close(cx);
            cx.notify();
            return;
        }
        let operation = self.sidebar_operation(&intent);
        self.sidebar_saving = Some(intent);
        #[cfg(test)]
        if let Some(io) = self.controls.preference_io.clone() {
            self.save_with(operation, move || (io.load)(), false, cx);
            return;
        }
        self.save_native(operation, cx);
    }

    /// Called by the save completion with the intent that save wrote. A newer
    /// draft made during the write stays live and queued.
    pub(super) fn finish_sidebar_save(
        &mut self,
        saved: Option<SidebarIntent>,
        ok: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(saved) = saved else {
            return false;
        };
        if !ok {
            return false;
        }
        // A newer draft stays live; only a commit (which sets
        // `sidebar_queued`) writes it, never a preview mid-drag.
        if self.sidebar_intent.as_ref() == Some(&saved) {
            self.sidebar_intent = None;
            clear_sidebar_draft(cx);
        }
        true
    }

    /// The write a quit still owes, unless the running save already
    /// writes this very draft. The file may hold the running save's values
    /// by then, so the quit writes every key rather than a diff against the
    /// last load: a revert to the loaded value must still land.
    pub(super) fn take_shutdown_sidebar(
        &mut self,
    ) -> Option<impl FnOnce() -> crate::Result<()> + Send + 'static + use<>> {
        let intent = self.sidebar_intent.take()?;
        if self.sidebar_saving.as_ref() == Some(&intent) {
            return None;
        }
        let earlier = std::iter::once(&self.loaded_config.sidebar_style)
            .chain(self.sidebar_saving.as_ref().map(|saving| &saving.style));
        Some(self.sidebar_write(all_preferences(&intent, earlier)))
    }

    /// One row per host: connected endpoints first in window order, then
    /// configured hosts that are not connected, alphabetically.
    pub(super) fn host_rows(&self, cx: &App) -> Vec<(String, Option<u32>, &'static str)> {
        let hosts = &self.config.sidebar_style.hosts;
        let mut rows: Vec<(String, Option<u32>, &'static str)> = Vec::new();
        if let Some(source) = self.source.upgrade() {
            for endpoint in &source.read(cx).endpoints {
                if !endpoint.live.status.is_connected()
                    || rows.iter().any(|(label, ..)| *label == endpoint.label)
                {
                    continue;
                }
                let color = hosts.get(&endpoint.label).copied();
                let hint = if color.is_some() {
                    ""
                } else {
                    "Connected, no colour set."
                };
                rows.push((endpoint.label.clone(), color, hint));
            }
        }
        // The map is sorted, so configured-only hosts come alphabetically.
        for (host, color) in hosts {
            if !rows.iter().any(|(label, ..)| label == host) {
                rows.push((host.clone(), Some(*color), "Not connected."));
            }
        }
        rows
    }

    pub(super) fn set_host_color(
        &mut self,
        host: &str,
        color: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        let mut intent = SidebarIntent::of(&self.config);
        match color {
            Some(color) => intent.style.hosts.insert(host.to_owned(), color),
            None => intent.style.hosts.remove(host),
        };
        self.commit_sidebar(intent, cx);
    }

    pub(super) fn begin_hex_edit(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.quitting || self.closing.is_some() {
            return;
        }
        if !self.finish_hex_edit(true, cx) {
            self.finish_hex_edit(false, cx);
        }
        let current = self
            .host_rows(cx)
            .get(index)
            .and_then(|(_, color, _)| *color)
            .map(|color| format!("{color:06x}"))
            .unwrap_or_default();
        let input = cx.new(SearchInput::new);
        input.update(cx, |input, cx| {
            input.set_text_selected(&current, cx);
            input.set_appearance(self.config.terminal.clone(), self.theme.clone(), cx);
        });
        let focus = input.read(cx).focus.clone();
        // Like Enter: an invalid colour keeps the field open and marked;
        // only Escape discards it.
        let blur = cx.on_blur(&focus, window, |this, _, cx| {
            this.finish_hex_edit(true, cx);
        });
        self.hex_edit = Some(HexEdit {
            index,
            input,
            invalid: false,
            _blur: blur,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Commits or abandons the typed colour. False means the text is not a
    /// colour (or is mid-composition) and the field stays open, marked.
    pub(super) fn finish_hex_edit(&mut self, save: bool, cx: &mut Context<Self>) -> bool {
        let Some(edit) = &mut self.hex_edit else {
            return true;
        };
        let color = if save {
            if edit.input.read(cx).is_composing() {
                return false;
            }
            let Some(color) = ui::parse_hex_input(edit.input.read(cx).text()) else {
                edit.invalid = true;
                cx.notify();
                return false;
            };
            Some(color)
        } else {
            None
        };
        let index = edit.index;
        self.hex_edit = None;
        if let Some(color) = color
            && let Some((host, ..)) = self.host_rows(cx).into_iter().nth(index)
        {
            self.set_host_color(&host, Some(color), cx);
        }
        cx.notify();
        true
    }
}
