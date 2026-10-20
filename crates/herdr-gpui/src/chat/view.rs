use super::{
    Error,
    input::{Composer, Submit},
    rows::{Body, Row},
    worker::{Identity, Reply, Request, Worker},
};
use crate::{
    config::{Config, Theme, corners},
    fonts::StyledFont,
    icons::AgentIcon,
    release_notes,
};
use gpui::{prelude::*, *};
use herdr_client::{
    ClientHandle, ConnectTarget,
    chat::{Conversation, Mode, Role},
    protocol::AgentStatus,
};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) struct View {
    pub mode: Mode,
    pub pane: String,
    pub label: String,
    config: Config,
    theme: Theme,
    focus: FocusHandle,
    terminal_focus: FocusHandle,
    composer: Entity<Composer>,
    _submit: Subscription,
    worker: Option<Worker>,
    identity: Option<Identity>,
    conversation: Option<Arc<Conversation>>,
    rows: Arc<[Row]>,
    shown: Vec<Row>,
    expanded: HashSet<String>,
    list: ListState,
    connected: bool,
    status: AgentStatus,
    loading: bool,
    sending: Option<String>,
    wanted: Option<(Identity, String)>,
    next_read: Instant,
    pages: usize,
    error: Option<String>,
    outcome: Option<&'static str>,
    replaced: bool,
}

impl View {
    #[cfg(any(test, feature = "mockup"))]
    pub(super) fn fixture(config: Config, theme: Theme, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let mut view = Self::new(
            "preview-pane".into(),
            "checkout-api / fix/empty-search".into(),
            config,
            theme,
            focus,
            cx,
        );
        let conversation = Arc::new(super::fixture::conversation());
        view.rows = super::rows::prepare(&conversation);
        view.conversation = Some(conversation);
        view.mode = Mode::Chat;
        view.connected = true;
        view.status = AgentStatus::Idle;
        view.rebuild();
        // No worker or writable identity: QA cannot submit to an actual agent.
        view
    }
    pub fn new(
        pane: String,
        label: String,
        config: Config,
        theme: Theme,
        terminal_focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let composer = cx.new(|cx| Composer::new(config.ui.clone(), theme.clone(), cx));
        let submit = cx.subscribe(&composer, |this, _, _: &Submit, cx| this.submit(cx));
        Self {
            mode: Mode::Terminal,
            pane,
            label,
            config,
            theme,
            terminal_focus,
            focus: cx.focus_handle(),
            composer,
            _submit: submit,
            worker: None,
            identity: None,
            conversation: None,
            rows: Arc::from([]),
            shown: Vec::new(),
            expanded: HashSet::new(),
            list: ListState::new(0, ListAlignment::Top, px(300.)),
            connected: false,
            status: AgentStatus::Unknown,
            loading: false,
            sending: None,
            wanted: None,
            next_read: Instant::now(),
            pages: 1,
            error: None,
            outcome: None,
            replaced: false,
        }
    }

    pub fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        if mode == Mode::Terminal {
            window.focus(&self.terminal_focus, cx);
        } else {
            let focus = self.composer.read(cx).focus.clone();
            window.focus(&focus, cx);
            self.next_read = Instant::now();
        }
        cx.notify();
    }

    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        let stopped = self.worker.take().is_some();
        if stopped || self.loading || self.wanted.is_some() || self.sending.is_some() {
            if self.sending.take().is_some() {
                self.error =
                    Some("Delivery was interrupted. Check Terminal before sending again.".into());
                self.outcome = Some("Your draft was kept.");
            }
            self.loading = false;
            self.wanted = None;
            cx.notify();
        }
    }

    pub fn can_forget(&self, cx: &App) -> bool {
        self.mode == Mode::Terminal
            && self.composer.read(cx).text().is_empty()
            && self.sending.is_none()
            && self.wanted.is_none()
    }

    pub fn saved_draft(&self, cx: &App) -> (Mode, String) {
        (self.mode, self.composer.read(cx).text().to_owned())
    }

    pub fn restore_detached_draft(&mut self, mode: Mode, draft: &str, cx: &mut Context<Self>) {
        self.mode = mode;
        self.composer
            .update(cx, |input, cx| input.restore(draft, cx));
        self.replaced = true;
        self.error = Some("The daemon connection changed. Your previous draft is kept here and will not be sent to the new session.".into());
    }

    pub fn appearance(&mut self, config: &Config, theme: &Theme, cx: &mut Context<Self>) {
        if self.config.ui != config.ui
            || self.config.terminal != config.terminal
            || &self.theme != theme
        {
            self.config = config.clone();
            self.theme = theme.clone();
            self.list.remeasure();
            self.composer.update(cx, |v, cx| {
                v.appearance(config.ui.clone(), theme.clone(), cx)
            });
            cx.notify();
        }
    }

    pub fn tick(
        &mut self,
        target: &ConnectTarget,
        handle: Option<ClientHandle>,
        connected: bool,
        status: AgentStatus,
        cx: &mut Context<Self>,
    ) {
        if self.connected != connected || self.status != status {
            self.connected = connected;
            self.status = status;
            cx.notify();
        }
        if !connected || status == AgentStatus::Unknown || self.mode != Mode::Chat {
            self.suspend(cx);
            return;
        }
        if self.replaced {
            return;
        }
        let now = Instant::now();
        if self.worker.is_none() && now >= self.next_read {
            if let Some(handle) = handle {
                match Worker::start(target, self.pane.clone(), handle) {
                    Ok(worker) => self.worker = Some(worker),
                    Err(e) => {
                        self.error = Some(e.to_string());
                        self.next_read = now + Duration::from_secs(5);
                        cx.notify();
                        return;
                    }
                }
            } else {
                return;
            }
        }
        let reply = match self.worker.as_ref().map(|w| w.rx.try_recv()) {
            Some(Ok(reply)) => Some(reply),
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.suspend(cx);
                self.error = Some(Error::Stopped.to_string());
                self.next_read = now + Duration::from_secs(5);
                cx.notify();
                None
            }
            _ => None,
        };
        match reply {
            Some(Reply::Read(result)) => {
                self.loading = false;
                match result {
                    Ok(snapshot) => {
                        let changed_session = self
                            .identity
                            .as_ref()
                            .is_some_and(|identity| *identity != snapshot.identity);
                        if changed_session {
                            // Never silently attach the old draft to a replacement session.
                            self.error = Some(Error::TargetChanged.to_string());
                            self.replaced = true;
                            self.worker = None;
                            cx.notify();
                            return;
                        }
                        self.identity = Some(snapshot.identity);
                        if self
                            .conversation
                            .as_ref()
                            .is_none_or(|old| !Arc::ptr_eq(old, &snapshot.conversation))
                        {
                            let reset = self
                                .conversation
                                .as_ref()
                                .is_some_and(|old| old.boundary != snapshot.conversation.boundary);
                            if reset {
                                self.expanded.clear();
                                self.shown.clear();
                                self.list.reset(0);
                            }
                            self.rows = snapshot.rows;
                            self.rebuild();
                            self.conversation = Some(snapshot.conversation);
                            if self.error.is_none() {
                                self.outcome = None;
                            }
                        }
                        // Submission errors survive a successful background read.
                        if self.outcome.is_none() {
                            self.error = None;
                        }
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                    }
                }
                cx.notify();
            }
            Some(Reply::Sent(result)) => {
                let text = self.sending.take();
                match result {
                    Ok(()) => {
                        if text.as_deref() == Some(self.composer.read(cx).text()) {
                            self.composer.update(cx, |v, cx| v.clear(cx));
                        }
                        self.error = None;
                        self.outcome =
                            Some("Sent to Terminal. Waiting for the transcript to update.");
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        self.outcome = Some("Your draft was kept.");
                    }
                }
                self.next_read = now;
                cx.notify();
            }
            None => {}
        }
        if self.loading || self.sending.is_some() {
            return;
        }
        if let Some((identity, text)) = self.wanted.take() {
            let request = Request::Send {
                identity,
                text: text.clone(),
            };
            match self
                .worker
                .as_ref()
                .ok_or(Error::Stopped)
                .and_then(|w| w.request(request))
            {
                Ok(()) => self.sending = Some(text),
                Err(e) => self.error = Some(e.to_string()),
            }
            cx.notify();
        } else if now >= self.next_read {
            self.next_read = now + Duration::from_millis(750);
            if let Some(worker) = &self.worker {
                match worker.request(Request::Read { pages: self.pages }) {
                    Ok(()) => self.loading = true,
                    Err(e) => {
                        self.error = Some(e.to_string());
                        self.worker = None;
                    }
                }
            }
        }
    }

    fn rebuild(&mut self) {
        let follow = self.at_end();
        let shown: Vec<_> = self
            .rows
            .iter()
            .filter(|row| {
                row.group
                    .as_ref()
                    .is_none_or(|group| self.expanded.contains(group))
            })
            .cloned()
            .collect();
        let prefix = self
            .shown
            .iter()
            .zip(&shown)
            .take_while(|(old, new)| old == new)
            .count();
        let suffix = self.shown[prefix..]
            .iter()
            .rev()
            .zip(shown[prefix..].iter().rev())
            .take_while(|(old, new)| old == new)
            .count();
        self.list.splice(
            prefix..self.shown.len() - suffix,
            shown.len() - prefix - suffix,
        );
        self.shown = shown;
        if follow {
            self.list.scroll_to_end();
        }
    }

    fn at_end(&self) -> bool {
        let count = self.list.item_count();
        if count == 0 || self.list.logical_scroll_top().item_ix >= count {
            return true;
        }
        self.list.is_scrolled_to_end().unwrap_or_else(|| {
            self.list
                .bounds_for_item(count - 1)
                .is_some_and(|last| last.bottom() <= self.list.viewport_bounds().bottom() + px(1.))
        })
    }

    fn can_send(&self, cx: &App) -> bool {
        self.connected
            && matches!(self.status, AgentStatus::Idle | AgentStatus::Done)
            && self.identity.is_some()
            && self.error.is_none()
            && self.sending.is_none()
            && self.wanted.is_none()
            && !self.composer.read(cx).composing()
    }
    fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.can_send(cx) {
            return;
        }
        let Some(identity) = self.identity.clone() else {
            return;
        };
        let text = self.composer.read(cx).text();
        if text.trim().is_empty() {
            return;
        }
        self.wanted = Some((identity, text.into()));
        self.outcome = None;
        cx.notify();
    }

    fn header(&self, cx: &Context<Self>) -> Div {
        let theme = &self.theme;
        let mode = div()
            .flex_none()
            .flex()
            .gap(px(2.))
            .p(px(3.))
            .rounded(px(corners::CONTROL))
            .bg(rgb(theme.active))
            .children(
                [Mode::Terminal, Mode::Chat]
                    .into_iter()
                    .enumerate()
                    .map(|(i, mode)| {
                        let selected = self.mode == mode;
                        div()
                            .id(("chat-mode", i))
                            .debug_selector(move || format!("agent-view-{mode:?}").to_lowercase())
                            .role(gpui::Role::Button)
                            .aria_label(if mode == Mode::Chat {
                                "Chat"
                            } else {
                                "Terminal"
                            })
                            .cursor_pointer()
                            .px_3()
                            .py_1()
                            .rounded(px(corners::SMALL))
                            .when(selected, |el| {
                                el.bg(rgb(theme.background))
                                    .shadow_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                            })
                            .child(if mode == Mode::Chat {
                                "Chat"
                            } else {
                                "Terminal"
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_mode(mode, window, cx)
                            }))
                    }),
            );
        let state = if !self.connected {
            "Disconnected"
        } else {
            match self.status {
                AgentStatus::Working => "Working",
                AgentStatus::Blocked => "Needs input",
                AgentStatus::Done => "Done",
                AgentStatus::Idle => "Ready",
                AgentStatus::Unknown => "Unknown",
            }
        };
        div()
            .flex_none()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(rgb(theme.active))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(
                        svg()
                            .path(AgentIcon::Claude.path())
                            .size(px(18.))
                            .text_color(rgb(theme.foreground)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Claude Code"),
                    )
                    .child(mode),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .text_size(px(self.config.ui.size - 1.))
                    .text_color(rgb(theme.subtext()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(format!("{} · {}", self.label, self.pane)),
                    )
                    .child(state),
            )
    }

    fn row(&self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.shown.get(i) else {
            return div().into_any_element();
        };
        let theme = &self.theme;
        let element = match &row.body {
            Body::Heading { role, copy } => {
                let copy = copy.clone();
                div()
                    .mt_4()
                    .mb_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if *role == Role::User { "You" } else { "Claude" }),
                    )
                    .child(
                        div()
                            .id(("chat-copy", i))
                            .text_size(px(11.))
                            .cursor_pointer()
                            .text_color(rgb(theme.subtext()))
                            .child("Copy")
                            .on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy.to_string()))
                            }),
                    )
                    .into_any_element()
            }
            Body::Text { role, line } => div()
                .py_1()
                .when(*role == Role::User, |el| {
                    el.px_3()
                        .rounded(px(corners::CONTROL))
                        .bg(rgb(theme.active))
                })
                .child(release_notes::line(
                    "chat-text",
                    i,
                    line,
                    theme,
                    &self.config.terminal,
                ))
                .into_any_element(),
            Body::Actions { group, count } => {
                let open = self.expanded.contains(group);
                let group = group.clone();
                div()
                    .id(("chat-actions", i))
                    .cursor_pointer()
                    .my_2()
                    .px_3()
                    .py_2()
                    .rounded(px(corners::CONTROL))
                    .bg(rgb(theme.surface))
                    .child(format!("{} {count} actions", if open { "▾" } else { "▸" }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded.remove(&group) && this.expanded.len() < 128 {
                            this.expanded.insert(group.clone());
                        }
                        this.rebuild();
                        cx.notify();
                    }))
                    .into_any_element()
            }
            Body::Tool { name, state } => div()
                .py_2()
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("{name} · {state}"))
                .into_any_element(),
            Body::Notice(text) => div()
                .py_2()
                .text_color(rgb(theme.subtext()))
                .child(text.clone())
                .into_any_element(),
        };
        div()
            .id(("chat-row", i))
            .debug_selector(move || format!("chat-row-{i}"))
            .w_full()
            .px_4()
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .mx_auto()
                    .min_w_0()
                    .child(element),
            )
            .into_any_element()
    }

    fn message(&self) -> Option<String> {
        if !self.connected {
            return Some(
                "Disconnected. History is a saved view; your draft will not be sent automatically."
                    .into(),
            );
        }
        if self.status == AgentStatus::Unknown {
            return Some(
                "This Claude pane is no longer ready. Switch to Terminal to inspect it.".into(),
            );
        }
        if self.status == AgentStatus::Blocked {
            return Some("Claude needs an answer. Switch to Terminal to continue.".into());
        }
        if let Some(error) = &self.error {
            return Some(error.clone());
        }
        if self.sending.is_some() {
            return Some("Sending to Terminal…".into());
        }
        if self.status == AgentStatus::Working {
            return Some(
                "Draft while Claude works. Send becomes available when it is ready.".into(),
            );
        }
        if self.conversation.is_none() {
            return Some("Reading Claude’s conversation…".into());
        }
        self.outcome.map(str::to_owned)
    }
}

impl Render for View {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let send = self.can_send(cx);
        div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .text_font(&self.config.ui)
            .text_size(px(self.config.ui.size))
            .line_height(px(self.config.ui.line_height()))
            .when(self.mode == Mode::Chat, |el| {
                el.flex_1().min_h_0().track_focus(&self.focus)
            })
            .child(self.header(cx))
            .when(self.mode == Mode::Chat, |el| {
                el.when(
                    self.conversation.as_ref().is_some_and(|c| c.limited),
                    |el| {
                        el.child(
                            div()
                                .id("chat-older")
                                .cursor_pointer()
                                .p_2()
                                .text_color(rgb(theme.subtext()))
                                .child("Load older history (bounded to 16 MiB)")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.pages = (this.pages + 1).min(8);
                                    this.next_read = Instant::now();
                                    cx.notify();
                                })),
                        )
                    },
                )
                .child(if self.shown.is_empty() && self.conversation.is_some() {
                    div()
                        .debug_selector(|| "chat-empty".into())
                        .flex_1()
                        .min_h_0()
                        .p_4()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            svg()
                                .path(AgentIcon::Claude.path())
                                .size(px(28.))
                                .text_color(rgb(theme.subtext())),
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("No messages recorded yet"),
                        )
                        .child(div().text_color(rgb(theme.subtext())).child(
                            "Send a message below to the Claude session running in this pane.",
                        ))
                        .into_any_element()
                } else {
                    list(
                        self.list.clone(),
                        cx.processor(|this, i: usize, _, cx| this.row(i, cx)),
                    )
                    .flex_1()
                    .min_h_0()
                    .py_2()
                    .into_any_element()
                })
                .when(!self.at_end(), |el| {
                    el.child(
                        div()
                            .id("chat-latest")
                            .cursor_pointer()
                            .p_2()
                            .child("↓ Latest messages")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.list.scroll_to_end();
                                cx.notify();
                            })),
                    )
                })
                .children(
                    self.message()
                        .map(|message| div().px_3().py_2().bg(rgb(theme.surface)).child(message)),
                )
                .when(self.error.is_some(), |el| {
                    el.child(
                        div()
                            .id("chat-retry")
                            .cursor_pointer()
                            .px_3()
                            .py_1()
                            .child(if self.replaced {
                                "Open the new session (discard old draft)"
                            } else {
                                "Retry reading / I checked Terminal"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.replaced {
                                    this.composer.update(cx, |v, cx| v.clear(cx));
                                    this.identity = None;
                                    this.conversation = None;
                                    this.rows = Arc::from([]);
                                    this.shown.clear();
                                    this.list.reset(0);
                                    this.expanded.clear();
                                    this.wanted = None;
                                    this.replaced = false;
                                }
                                this.error = None;
                                this.outcome = None;
                                this.next_read = Instant::now();
                                cx.notify();
                            })),
                    )
                })
                .child(
                    div().p_3().child(
                        div()
                            .p_3()
                            .rounded(px(corners::PANEL))
                            .border_1()
                            .border_color(rgb(theme.active))
                            .bg(rgb(theme.surface))
                            .child(self.composer.clone())
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(px(11.))
                                            .text_color(rgb(theme.subtext()))
                                            .child("Enter: new line · Cmd/Ctrl+Enter: send"),
                                    )
                                    .child(
                                        div()
                                            .id("chat-send")
                                            .debug_selector(|| "chat-send".into())
                                            .px_3()
                                            .py_1()
                                            .rounded(px(corners::CONTROL))
                                            .bg(rgb(theme.primary_wash()))
                                            .when(!send, |el| el.opacity(0.5))
                                            .child("Send ↑")
                                            .when(send, |el| {
                                                el.cursor_pointer().on_click(
                                                    cx.listener(|this, _, _, cx| this.submit(cx)),
                                                )
                                            }),
                                    ),
                            ),
                    ),
                )
            })
    }
}

#[cfg(test)]
mod tests;
