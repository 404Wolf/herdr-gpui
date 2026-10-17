//! A section's page: the repository and its sources, search and filters,
//! and the list, with an item's details in a pane beside it once opened.
//! Issues keep their filters in a column that stays folded until asked for,
//! the active ones showing as chips. Lists are virtualized, so thousands of
//! beads cost only the rows on screen.

use super::{
    Event, OrchestratorView, PANE_MAX_SHARE, PANE_MIN, PaneDrag,
    look::{BLUE, GREEN, Look, MAGENTA, RED, YELLOW, age},
    rows::{Sort, Tab},
};
use crate::{
    config::corners,
    orchestrator::{Access, Provider, SyncState},
};
use gpui::{prelude::*, *};

impl OrchestratorView {
    pub(super) fn render_inbox(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pane = self
            .detail
            .clone()
            .map(|detail| self.render_pane(detail, window, cx));
        let filters =
            (self.tab == Tab::Issues && self.filters_open).then(|| self.render_facets(cx));
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.render_header(cx))
            .child(self.render_search_row(window, cx))
            .children(self.render_chips(cx))
            .children(self.render_banners(cx))
            .child(
                div()
                    .id("orchestrator-body")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .on_drag_move(cx.listener(
                        |this, event: &DragMoveEvent<PaneDrag>, window, cx| {
                            let right = event.bounds.right();
                            let width = f32::from(right - event.event.position.x);
                            let max = f32::from(window.viewport_size().width) * PANE_MAX_SHARE;
                            // Dragged nearly shut, the details close.
                            if width < PANE_MIN / 2. {
                                this.detail = None;
                            } else {
                                this.pane_width = width.clamp(PANE_MIN, max.max(PANE_MIN));
                            }
                            cx.notify();
                        },
                    ))
                    .children(filters)
                    .child(self.render_list(cx))
                    .children(pane),
            )
            .when(self.sort_open, |el| el.child(self.render_sort_menu(cx)))
            .into_any_element()
    }

    /// The open item's page beside the list; its left edge drags to resize
    /// it, or shut.
    fn render_pane(
        &mut self,
        detail: super::Detail,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.look.theme;
        let handle = div()
            .id("orchestrator-pane-handle")
            .flex_none()
            .w(px(6.))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .border_l_1()
            .border_color(rgb(theme.active))
            .cursor(CursorStyle::ResizeLeftRight)
            .on_drag(PaneDrag, |drag, _, _, cx| cx.new(|_| *drag))
            .child(
                div()
                    .w(px(2.))
                    .h(px(28.))
                    .rounded_full()
                    .bg(rgb(theme.active)),
            );
        div()
            .debug_selector(|| "orchestrator-pane".into())
            .flex_none()
            .w(px(self.pane_width))
            .h_full()
            .flex()
            .child(handle)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(self.render_detail(detail, window, cx)),
            )
    }

    /// How many of the Issues filters narrow the list.
    fn active_filters(&self) -> usize {
        usize::from(self.filters.provider.is_some())
            + usize::from(self.filters.has_run)
            + usize::from(self.filters.blocked)
            + usize::from(self.label.is_some())
    }

    /// The Issues filters' column: state, source, and the labels the open
    /// issues carry, most used first.
    fn render_facets(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let look = &self.look;
        let theme = &look.theme;
        let items = &self.snapshot.items;
        let open: Vec<&crate::orchestrator::Item> = items
            .iter()
            .filter(|item| item.pull_request.is_none() && !super::rows::closed(item))
            .collect();
        let mut labels: Vec<(String, usize)> = Vec::new();
        for label in open.iter().flat_map(|item| &item.labels) {
            match labels.iter_mut().find(|(name, _)| name == label) {
                Some((_, count)) => *count += 1,
                None => labels.push((label.clone(), 1)),
            }
        }
        labels.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        labels.truncate(FACET_LABELS);
        let hover = theme.surface;
        let option = |id: SharedString, label: SharedString, count: Option<usize>, on: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py(px(3.))
                .rounded(px(corners::SMALL))
                .cursor_pointer()
                .hover(move |style| style.bg(rgb(hover)))
                .when(on, |el| el.bg(rgb(theme.primary_wash())))
                .child(
                    div()
                        .flex_none()
                        .size(px(11.))
                        .rounded(px(2.))
                        .border_1()
                        .border_color(rgb(if on { theme.primary() } else { theme.muted }))
                        .when(on, |el| el.bg(rgb(theme.primary()))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(look.small())
                        .child(label),
                )
                .children(count.map(|count| look.muted(count.to_string())))
        };
        let heading = |text: &'static str| look.label(text).pt_2().pb_1();
        let has_run = open
            .iter()
            .filter(|item| {
                self.snapshot
                    .runs
                    .iter()
                    .any(|run| run.item_key == item.key.canonical())
            })
            .count();
        let blocked = open
            .iter()
            .filter(|item| !item.blocked_by.is_empty() || item.state == "blocked")
            .count();
        let providers: Vec<Provider> = {
            let mut found: Vec<Provider> = self
                .snapshot
                .sources
                .iter()
                .map(|source| source.key.provider)
                .collect();
            found.dedup();
            found
        };
        div()
            .id("orchestrator-facets")
            .debug_selector(|| "orchestrator-facets".into())
            .flex_none()
            .w(px(FACETS_WIDTH))
            .h_full()
            .overflow_y_scroll()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(rgb(theme.active))
            .child(heading("STATE"))
            .child(
                option(
                    "orchestrator-facet-run".into(),
                    "Has agent".into(),
                    Some(has_run),
                    self.filters.has_run,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.filters.has_run = !this.filters.has_run;
                    this.refresh_rows();
                    cx.notify();
                })),
            )
            .child(
                option(
                    "orchestrator-facet-blocked".into(),
                    "Blocked".into(),
                    Some(blocked),
                    self.filters.blocked,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.filters.blocked = !this.filters.blocked;
                    this.refresh_rows();
                    cx.notify();
                })),
            )
            .when(providers.len() > 1, |el| {
                el.child(heading("SOURCE"))
                    .children(providers.into_iter().map(|provider| {
                        let on = self.filters.provider == Some(provider);
                        let count = open
                            .iter()
                            .filter(|item| item.key.source.provider == provider)
                            .count();
                        option(
                            format!("orchestrator-facet-source-{}", provider_name(provider)).into(),
                            provider_name(provider).into(),
                            Some(count),
                            on,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.filters.provider =
                                (this.filters.provider != Some(provider)).then_some(provider);
                            this.refresh_rows();
                            cx.notify();
                        }))
                    }))
            })
            .when(!labels.is_empty(), |el| {
                el.child(heading("LABELS"))
                    .children(labels.into_iter().map(|(label, count)| {
                        let on = self.label.as_deref() == Some(label.as_str());
                        let picked = label.clone();
                        option(
                            format!("orchestrator-facet-label-{label}").into(),
                            label.into(),
                            Some(count),
                            on,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.label = (this.label.as_deref() != Some(picked.as_str()))
                                .then(|| picked.clone());
                            this.refresh_rows();
                            cx.notify();
                        }))
                    }))
            })
    }

    /// While the Issues filters are folded, the ones in use as chips that
    /// clear them.
    fn render_chips(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.tab != Tab::Issues || self.filters_open || self.active_filters() == 0 {
            return None;
        }
        let look = &self.look;
        let theme = &look.theme;
        let chip = |id: &'static str, text: String, clear: fn(&mut Self)| {
            div()
                .id(id)
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .px_2()
                .py(px(1.))
                .rounded_full()
                .bg(rgb(theme.primary_wash()))
                .cursor_pointer()
                .text_size(look.small())
                .child(text)
                .child(look.icon("icons/close.svg", 9., theme.muted))
                .on_click(cx.listener(move |this, _, _, cx| {
                    clear(this);
                    this.refresh_rows();
                    cx.notify();
                }))
        };
        let mut row = div()
            .debug_selector(|| "orchestrator-chips".into())
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_4()
            .pb_2()
            .child(look.muted("Showing only"));
        if let Some(provider) = self.filters.provider {
            row = row.child(chip(
                "orchestrator-chip-source",
                provider_name(provider).into(),
                |this| {
                    this.filters.provider = None;
                },
            ));
        }
        if self.filters.has_run {
            row = row.child(chip("orchestrator-chip-run", "Has agent".into(), |this| {
                this.filters.has_run = false;
            }));
        }
        if self.filters.blocked {
            row = row.child(chip(
                "orchestrator-chip-blocked",
                "Blocked".into(),
                |this| {
                    this.filters.blocked = false;
                },
            ));
        }
        if let Some(label) = &self.label {
            row = row.child(chip("orchestrator-chip-label", label.clone(), |this| {
                this.label = None;
            }));
        }
        Some(row)
    }

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let look = &self.look;
        let theme = &look.theme;
        let info = self.snapshot.repo.as_ref();
        let name = info
            .and_then(|info| info.remote.as_ref())
            .map(|remote| remote.source.repository.clone())
            .or_else(|| {
                info.map(|info| {
                    info.main_root
                        .rsplit('/')
                        .next()
                        .unwrap_or(&info.main_root)
                        .to_owned()
                })
            })
            .unwrap_or_else(|| "Reading the repository\u{2026}".into());
        let host = match &self.request.target {
            herdr_client::ConnectTarget::Ssh { target, .. } => target.clone(),
            _ => "local".into(),
        };
        let sources = self
            .snapshot
            .sources
            .iter()
            .enumerate()
            .map(|(index, status)| {
                let (text, color, sign_in) = match &status.state {
                    SyncState::Syncing => ("syncing\u{2026}".to_owned(), look.hue(BLUE), false),
                    SyncState::Synced { count, at } => (
                        format!("{count} \u{00b7} {}", synced(*at)),
                        look.hue(GREEN),
                        false,
                    ),
                    SyncState::Capped { count } => {
                        (format!("first {count}"), look.hue(YELLOW), false)
                    }
                    SyncState::SignIn => ("sign in".to_owned(), look.hue(YELLOW), true),
                    SyncState::Unsupported => ("not supported yet".to_owned(), theme.muted, false),
                    SyncState::Failed(_) => ("failed".to_owned(), look.hue(RED), false),
                };
                let provider = status.key.provider;
                div()
                    .id(("orchestrator-source", index))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py(px(2.))
                    .rounded(px(corners::CONTROL))
                    .border_1()
                    .border_color(rgb(theme.active))
                    .text_size(look.small())
                    .child(match provider {
                        Provider::Github => look
                            .icon("icons/github.svg", 12., theme.subtext())
                            .into_any_element(),
                        Provider::Beads => look.badge("bd", MAGENTA).into_any_element(),
                        Provider::Gitlab => look.badge("gl", RED).into_any_element(),
                    })
                    .child(match provider {
                        Provider::Github => "GitHub",
                        Provider::Beads => "Beads",
                        Provider::Gitlab => "GitLab",
                    })
                    .child(look.dot(color, 6.))
                    .child(div().text_color(rgb(theme.muted)).child(text))
                    .when(sign_in, |el| {
                        el.cursor_pointer()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(Event::SignIn)))
                    })
            });
        let dispatchable =
            self.selected.is_some() && self.snapshot.access == Some(Access::ReadWrite);
        div()
            .flex_none()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .child(look.icon("icons/git-branch.svg", 14., theme.subtext()))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
            .child(look.chip(host))
            .children(sources)
            .child(div().flex_1())
            .child(
                look.icon_button("orchestrator-refresh", "icons/refresh.svg")
                    .on_click(cx.listener(|this, _, _, _| this.refresh())),
            )
            .when(dispatchable, |el| {
                el.child(
                    look.button("orchestrator-dispatch", "Dispatch agent", true)
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(item) = this.selected.clone() {
                                this.open_dispatch(item, window, cx);
                            }
                        })),
                )
            })
    }

    fn render_search_row(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let look = &self.look;
        let theme = &look.theme;
        let filters = self.filters;
        let mut row = div()
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .child(self.render_search_field(window, cx));
        let toggle = |id: &'static str, label: &'static str, on: bool, flip: fn(&mut Self)| {
            look.toggle(id, label, on)
                .on_click(cx.listener(move |this, _, _, cx| {
                    flip(this);
                    this.refresh_rows();
                    cx.notify();
                }))
        };
        match self.tab {
            Tab::Issues => {
                let active = self.active_filters();
                let open = self.filters_open;
                row = row.child(
                    div()
                        .id("orchestrator-filters")
                        .debug_selector(|| "orchestrator-filters".into())
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py(px(3.))
                        .rounded(px(corners::CONTROL))
                        .border_1()
                        .border_color(rgb(if open { theme.primary() } else { theme.active }))
                        .when(open, |el| el.bg(rgb(theme.primary_wash())))
                        .cursor_pointer()
                        .text_size(look.small())
                        .child(look.icon("icons/panel-left.svg", 12., theme.subtext()))
                        .child("Filters")
                        .when(active > 0, |el| {
                            el.child(
                                div()
                                    .px(px(5.))
                                    .rounded_full()
                                    .bg(rgb(theme.primary()))
                                    .text_color(rgb(theme.text_on(theme.primary())))
                                    .text_size(look.tiny())
                                    .child(active.to_string()),
                            )
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.filters_open = !this.filters_open;
                            cx.notify();
                        })),
                );
            }
            Tab::PullRequests => {
                row = row
                    .child(toggle(
                        "orchestrator-open",
                        "Open",
                        filters.open_only,
                        |this| {
                            this.filters.open_only = !this.filters.open_only;
                        },
                    ))
                    .when(self.login.is_some(), |row| {
                        row.child(toggle("orchestrator-mine", "Mine", self.mine, |this| {
                            this.mine = !this.mine;
                        }))
                    })
                    .child(toggle(
                        "orchestrator-pr-has-run",
                        "Has agent",
                        filters.has_run,
                        |this| {
                            this.filters.has_run = !this.filters.has_run;
                        },
                    ));
            }
        }
        row.child(
            div()
                .id("orchestrator-sort")
                .flex_none()
                .flex()
                .items_center()
                .gap_1()
                .cursor_pointer()
                .text_size(look.small())
                .text_color(rgb(theme.subtext()))
                .child(self.sort.label())
                .child(look.icon("icons/chevron-down.svg", 10., theme.muted))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.sort_open = !this.sort_open;
                    cx.notify();
                })),
        )
    }

    /// One outlined field, the magnifier inside: the outline turns accent
    /// while the field has the keyboard, and a clear button shows once there
    /// is text.
    fn render_search_field(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let look = &self.look;
        let theme = &look.theme;
        let focused = self.search.read(cx).focus.is_focused(window);
        div()
            .id("orchestrator-search")
            .flex_1()
            .min_w_0()
            .h(px(look.ui.line_height() + 12.))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .rounded(px(corners::CONTROL))
            .border_1()
            .border_color(rgb(if focused {
                theme.primary()
            } else {
                theme.active
            }))
            .bg(rgb(theme.background))
            .child(look.icon(
                "icons/search.svg",
                13.,
                if focused {
                    theme.foreground
                } else {
                    theme.muted
                },
            ))
            .child(div().flex_1().min_w_0().child(self.search.clone()))
            .when(!self.query.is_empty(), |field| {
                field.child(
                    look.icon_button("orchestrator-search-clear", "icons/close.svg")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.search.update(cx, |search, cx| search.clear(cx));
                            window.focus(&this.search.read(cx).focus.clone(), cx);
                        })),
                )
            })
    }

    fn render_sort_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let theme = &look.theme;
        deferred(
            div()
                .absolute()
                .top(px(120.))
                .right(px(16.))
                .w(px(200.))
                .p_1()
                .flex()
                .flex_col()
                .rounded(px(corners::CONTROL))
                .bg(rgb(theme.surface))
                .border_1()
                .border_color(rgb(theme.active))
                .shadow_lg()
                .children(Sort::ALL.into_iter().map(|sort| {
                    let on = sort == self.sort;
                    let hover = theme.active;
                    div()
                        .id(SharedString::from(format!(
                            "orchestrator-sort-{}",
                            sort.label()
                        )))
                        .px_2()
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .rounded(px(corners::SMALL))
                        .cursor_pointer()
                        .hover(move |style| style.bg(rgb(hover)))
                        .when(on, |el| el.bg(rgb(theme.primary_wash())))
                        .child(sort.label())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.sort = sort;
                            this.sort_open = false;
                            this.refresh_rows();
                            cx.notify();
                        }))
                })),
        )
    }

    fn render_banners(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let look = &self.look;
        let mut banners = Vec::new();
        if let Some(Access::ReadOnly { found }) = self.snapshot.access {
            banners.push(banner(
                look,
                RED,
                "icons/lock.svg",
                format!(
                    "Shared data is schema {found}; this herdr-gpui understands up to {}. Read-only until updated.",
                    crate::orchestrator::SCHEMA_VERSION
                ),
            ));
        }
        if let Some(error) = &self.snapshot.error {
            banners.push(banner(look, RED, "icons/x.svg", error.to_string()));
        }
        for status in &self.snapshot.sources {
            let text = match &status.state {
                SyncState::Failed(error) => {
                    format!("{}: {error}", provider_name(status.key.provider))
                }
                SyncState::Capped { count } => format!(
                    "{} has more than {count} items; the newest are shown and not cached.",
                    provider_name(status.key.provider)
                ),
                _ => continue,
            };
            let hue = if matches!(status.state, SyncState::Capped { .. }) {
                YELLOW
            } else {
                RED
            };
            banners.push(banner(look, hue, "icons/refresh.svg", text));
        }
        banners
            .into_iter()
            .enumerate()
            .map(|(index, banner)| {
                banner
                    .id(("orchestrator-banner", index))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, _| this.refresh()))
                    .into_any_element()
            })
            .collect()
    }
}

/// The filters' column width, and the most labels it offers.
const FACETS_WIDTH: f32 = 200.;
const FACET_LABELS: usize = 12;

/// "synced now", "synced 4m ago": when a source last finished a sync.
fn synced(at: chrono::DateTime<chrono::Utc>) -> String {
    match age(Some(at), chrono::Utc::now()).as_str() {
        "now" => "synced now".into(),
        ago => format!("synced {ago} ago"),
    }
}

fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Github => "GitHub",
        Provider::Beads => "Beads",
        Provider::Gitlab => "GitLab",
    }
}

pub(super) fn banner(look: &Look, hue: usize, icon: &'static str, text: String) -> Div {
    div()
        .flex_none()
        .mx_4()
        .mb_2()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(corners::CONTROL))
        .bg(rgb(look.wash(hue)))
        .child(look.icon(icon, 13., look.hue(hue)))
        .child(div().flex_1().min_w_0().text_size(look.small()).child(text))
}
