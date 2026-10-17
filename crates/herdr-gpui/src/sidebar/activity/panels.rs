//! Drawing the list panels: a filter field over sections of two-line rows,
//! the device footer under them, and the edge that resizes the panel as the
//! Workspaces panel's does. Rows come from `rows`; clicking one opens what
//! it lists in its full view.

use super::{
    ActivityPanel,
    rows::{self, BranchPr, Opens, Row, Section, Source, Tone},
};
use crate::fonts::StyledFont;
use crate::{
    HerdrWindow,
    config::corners,
    sidebar::{SidebarDrag, line_height, sidebar_width},
};
use gpui::{prelude::*, *};

/// The list panels' rows, built once per sidebar render so the bar's counts
/// and the shown panel agree.
#[derive(Debug, Default)]
pub(super) struct Lists {
    orchestrator: Vec<Section>,
    pull_requests: Vec<Section>,
    issues: Vec<Section>,
    active_runs: usize,
    /// How many orchestrator views fed the lists: with none, the
    /// orchestrator's panels offer to open one.
    sources: usize,
}

impl Lists {
    /// The bar's count for `panel`, if it shows one.
    pub(super) fn count(&self, panel: ActivityPanel) -> Option<usize> {
        match panel {
            ActivityPanel::Workspaces => None,
            ActivityPanel::Orchestrator => Some(self.active_runs),
            ActivityPanel::PullRequests => Some(rows::count(&self.pull_requests)),
            ActivityPanel::Issues => Some(rows::count(&self.issues)),
        }
    }

    fn sections(&self, panel: ActivityPanel) -> &[Section] {
        match panel {
            ActivityPanel::Workspaces => &[],
            ActivityPanel::Orchestrator => &self.orchestrator,
            ActivityPanel::PullRequests => &self.pull_requests,
            ActivityPanel::Issues => &self.issues,
        }
    }
}

impl HerdrWindow {
    pub(super) fn activity_lists(&self, cx: &App) -> Lists {
        let mut views = self.orchestrator_views();
        views.sort_unstable_by_key(|(tab, _)| *tab);
        let statuses: Vec<_> = views
            .iter()
            .map(|(_, view)| view.read(cx).run_statuses())
            .collect();
        let sources: Vec<Source> = views
            .iter()
            .zip(&statuses)
            .map(|((tab, view), statuses)| {
                let view = view.read(cx);
                Source {
                    tab: *tab,
                    items: view.items(),
                    runs: view.runs(),
                    statuses,
                }
            })
            .collect();
        // The prefetch holds only the shown device's lookups, keyed as it
        // looked them up, local checkouts without worktree metadata included.
        let cache = &self.menu.pr_cache;
        let origin = self.pr_origin();
        let found: Vec<_> = origin
            .iter()
            .flat_map(|origin| {
                self.live
                    .snapshot
                    .iter()
                    .flat_map(|snapshot| &snapshot.workspaces)
                    .filter_map(move |workspace| {
                        let input = crate::pull_request::workspace_input(workspace, origin).ok()?;
                        Some((workspace.workspace_id.as_str(), input))
                    })
            })
            .filter_map(|(workspace, input)| Some((workspace, cache.get(&input)?)))
            .collect();
        let branches: Vec<BranchPr> = found
            .iter()
            .map(|(workspace, pr)| BranchPr { workspace, pr })
            .collect();
        Lists {
            orchestrator: rows::orchestrator(&sources),
            pull_requests: rows::pull_requests(&branches, &sources),
            issues: rows::issues(&sources),
            active_runs: rows::active_runs(&sources),
            sources: sources.len(),
        }
    }

    pub(super) fn render_list_panel(
        &self,
        panel: ActivityPanel,
        lists: &Lists,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let width = sidebar_width(self.sidebar_width, f32::from(window.viewport_size().width));
        let theme = &self.theme;
        let font = &self.config.sidebar;
        let filter = self.activity.filter.clone();
        filter.update(cx, |input, cx| {
            input.set_appearance(font.clone(), theme.clone(), cx);
            input.set_placeholder(panel.placeholder(), cx);
        });
        let query = filter.read(cx).text().to_owned();
        let sections = rows::filter(lists.sections(panel).to_vec(), &query);
        let mut list = div()
            .id("activity-list")
            .debug_selector(|| "activity-list".into())
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .pb(px(6.))
            .overflow_y_scroll();
        if sections.is_empty() {
            list = list.child(self.empty_panel(panel, lists, !query.trim().is_empty(), cx));
        }
        for (index, section) in sections.into_iter().enumerate() {
            list = list.child(self.section_heading(&section));
            for (row_index, row) in section.rows.into_iter().enumerate() {
                list = list.child(self.panel_row(row, (index, row_index), cx));
            }
        }
        div()
            .id("activity-panel")
            .debug_selector(|| "activity-panel".into())
            .relative()
            .w(px(width))
            .flex_none()
            .h_full()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .text_font(font)
            .text_size(px(font.size))
            .line_height(px(line_height(font)))
            .text_color(rgb(theme.foreground))
            .bg(self.fill().panel(theme.sidebar_background()))
            .border_r_1()
            .border_color(rgb(theme.active))
            .child(self.filter_field(filter))
            .child(list)
            .map(|panel| {
                if self.device_footer_shown() {
                    panel.child(self.render_device_footer(cx))
                } else {
                    panel.child(self.render_sessions_anchor())
                }
            })
            .child(self.panel_resize(width, cx))
    }

    fn filter_field(&self, filter: Entity<crate::search_input::SearchInput>) -> Div {
        let theme = &self.theme;
        div()
            .debug_selector(|| "activity-filter".into())
            .flex_none()
            .mx(px(8.))
            .mb(px(4.))
            .px(px(8.))
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(corners::CONTROL))
            .border_1()
            .border_color(rgb(theme.active))
            .child(
                svg()
                    .path("icons/search.svg")
                    .size(px(12.))
                    .flex_none()
                    .text_color(rgb(theme.muted)),
            )
            .child(div().flex_1().min_w_0().child(filter))
    }

    fn section_heading(&self, section: &Section) -> Div {
        let theme = &self.theme;
        div()
            .mt(px(8.))
            .mb(px(2.))
            .px(px(16.))
            .flex()
            .items_center()
            .gap(px(4.))
            .text_size(px(self.config.sidebar.size - 1.))
            .text_color(rgb(theme.muted))
            .child(section.title)
            .child(section.rows.len().to_string())
    }

    fn panel_row(&self, row: Row, at: (usize, usize), cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let hover = theme.active;
        let small = self.config.sidebar.size - 2.;
        let Row {
            number,
            title,
            meta,
            tag,
            opens,
        } = row;
        div()
            .id(ElementId::NamedInteger(
                "activity-row".into(),
                (at.0 * rows::ROW_LIMIT + at.1) as u64,
            ))
            .debug_selector(move || format!("activity-row-{}-{}", at.0, at.1))
            .mx(px(8.))
            .px(px(8.))
            .py(px(5.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .rounded(px(corners::CONTROL))
            .cursor_pointer()
            .hover(move |style| style.bg(rgb(hover)))
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .min_w_0()
                    .when(!number.is_empty(), |line| {
                        line.child(div().flex_none().text_color(rgb(theme.muted)).child(number))
                    })
                    .child(div().min_w_0().truncate().child(title)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(small))
                    .text_color(rgb(theme.muted))
                    .child(div().flex_1().min_w_0().truncate().child(meta))
                    .when_some(tag, |line, tag| {
                        let color = self.tone_color(tag.tone);
                        line.child(
                            div()
                                .flex_none()
                                .px(px(5.))
                                .rounded(px(corners::SMALL))
                                .border_1()
                                .border_color(rgb(crate::config::mix(
                                    theme.sidebar_background(),
                                    color,
                                    45,
                                )))
                                .text_color(rgb(color))
                                .child(tag.label),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.open_activity_row(opens.clone(), event.position(), window, cx);
            }))
    }

    fn tone_color(&self, tone: Tone) -> u32 {
        let theme = &self.theme;
        match tone {
            Tone::Attention => theme.ink(theme.palette[1]),
            Tone::Busy => theme.ink(theme.palette[3]),
            Tone::Good => theme.ink(theme.palette[2]),
            Tone::Quiet => theme.muted,
        }
    }

    /// Why the panel is empty, with the way to fill it when there is one.
    fn empty_panel(
        &self,
        panel: ActivityPanel,
        lists: &Lists,
        filtered: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = &self.theme;
        let needs_orchestrator = lists.sources == 0 && panel != ActivityPanel::PullRequests;
        let text = match (filtered, panel) {
            (true, _) => "No matches",
            (false, _) if needs_orchestrator => "Open the orchestrator to list them here",
            (false, ActivityPanel::Orchestrator) => "No agent runs",
            (false, ActivityPanel::PullRequests) => "No pull requests for these checkouts",
            (false, _) => "No open issues",
        };
        let hover = theme.active;
        div()
            .debug_selector(|| "activity-empty".into())
            .px(px(16.))
            .pt(px(12.))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(8.))
            .text_color(rgb(theme.muted))
            .child(text)
            .when(needs_orchestrator && !filtered, |empty| {
                empty.child(
                    div()
                        .id("activity-open-orchestrator")
                        .debug_selector(|| "activity-open-orchestrator".into())
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(px(corners::CONTROL))
                        .border_1()
                        .border_color(rgb(theme.active))
                        .text_color(rgb(theme.foreground))
                        .cursor_pointer()
                        .hover(move |style| style.bg(rgb(hover)))
                        .child("Open Orchestrator")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_orchestrator(window, cx);
                        })),
                )
            })
    }

    fn open_activity_row(
        &mut self,
        opens: Opens,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match opens {
            Opens::Workspace(workspace) => {
                self.navigate(crate::NavigationTarget::Workspace(&workspace), cx);
                self.open_workspace_menu(&workspace, anchor, window, cx);
            }
            Opens::Item { tab, key } => {
                if let Some(view) = self.show_orchestrator_tab(tab, window, cx) {
                    view.update(cx, |view, cx| view.reveal_item(key, cx));
                }
            }
            Opens::Run { tab, id } => {
                if let Some(view) = self.show_orchestrator_tab(tab, window, cx) {
                    view.update(cx, |view, cx| view.reveal_run(id, cx));
                }
            }
        }
    }

    /// The panel's right edge, which resizes it as the Workspaces panel's
    /// edge does: they share one width.
    fn panel_resize(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        div()
            .absolute()
            .top_0()
            .right_0()
            .h_full()
            .w(px(6.))
            .child(
                div()
                    .id("activity-resize")
                    .debug_selector(|| "activity-resize".into())
                    .size_full()
                    .cursor(CursorStyle::ResizeLeftRight)
                    .hover(|style| style.bg(rgba(crate::panel_resize::RESIZE_HOVER)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            this.sidebar_modified = true;
                            if event.click_count == 2 {
                                this.sidebar_drag = None;
                                this.sidebar_width = None;
                                this.save_sidebar_width();
                            } else {
                                this.sidebar_drag = Some(SidebarDrag::Width {
                                    start: f32::from(event.position.x),
                                    width,
                                });
                            }
                            cx.notify();
                        }),
                    ),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        // Captured globally, as the Workspaces panel does, so
                        // the drag continues past the narrow edge.
                        let moving = view.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase != DispatchPhase::Capture {
                                return;
                            }
                            let _ = moving.update(cx, |this, cx| {
                                let Some(SidebarDrag::Width { start, width }) = this.sidebar_drag
                                else {
                                    return;
                                };
                                this.sidebar_width = Some(sidebar_width(
                                    Some(width + f32::from(event.position.x) - start),
                                    f32::from(window.viewport_size().width),
                                ));
                                cx.stop_propagation();
                                cx.notify();
                            });
                        });
                        let released = view.clone();
                        window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                            if phase != DispatchPhase::Capture || event.button != MouseButton::Left
                            {
                                return;
                            }
                            let _ = released.update(cx, |this, cx| {
                                if this.sidebar_drag.take().is_some() {
                                    this.save_chrome();
                                    cx.stop_propagation();
                                    cx.notify();
                                }
                            });
                        });
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}
