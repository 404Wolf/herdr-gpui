//! The activity bar: an icon column at the expanded sidebar's left edge that
//! picks what the panel beside it lists. Workspaces is the sidebar as it was;
//! Orchestrator, Pull requests, and Issues list what the window already holds
//! (see `rows`). `[sidebar] activity_bar = false` turns the bar off, and the
//! collapsed rail never shows it.

use super::{agents::Indicators, line_height};
use crate::fonts::StyledFont;
use crate::{
    Command, HerdrWindow,
    browser::TabId,
    orchestrator::OrchestratorView,
    search_input::{Changed, SearchInput},
};
use gpui::{prelude::*, *};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

mod panels;
mod rows;

#[cfg(test)]
mod tests;

/// The bar's width: a 32px button with a margin either side.
pub(crate) const BAR_WIDTH: f32 = 44.;
const BUTTON: f32 = 32.;
const ICON: f32 = 17.;
const TITLE_HEIGHT: f32 = 32.;

/// What the panel beside the bar shows. The bar lists them in declaration
/// order, so this order is what users see.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivityPanel {
    #[default]
    Workspaces,
    Orchestrator,
    PullRequests,
    Issues,
}

impl ActivityPanel {
    pub(crate) const ALL: &[Self] = &[
        Self::Workspaces,
        Self::Orchestrator,
        Self::PullRequests,
        Self::Issues,
    ];

    /// The name the CLI and the preferences file use.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Workspaces => "workspaces",
            Self::Orchestrator => "orchestrator",
            Self::PullRequests => "pull-requests",
            Self::Issues => "issues",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|panel| panel.name() == name)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Workspaces => "Workspaces",
            Self::Orchestrator => "Orchestrator",
            Self::PullRequests => "Pull requests",
            Self::Issues => "Issues",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Workspaces => "icons/terminal.svg",
            Self::Orchestrator => "icons/fan-out.svg",
            Self::PullRequests => "icons/pull-request.svg",
            Self::Issues => "icons/issue.svg",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::Workspaces => "",
            Self::Orchestrator => "Filter runs",
            Self::PullRequests => "Filter pull requests",
            Self::Issues => "Filter issues",
        }
    }
}

/// The window's bar: the panel it shows and the list panels' filter, which
/// one field serves since one panel shows at a time.
pub(crate) struct Activity {
    pub(crate) panel: ActivityPanel,
    /// Picked in this window, so a stored panel arriving later yields to it.
    picked: bool,
    filter: Entity<SearchInput>,
    /// Each orchestrator view's lists as last drawn, by identity, so a
    /// view's new snapshot redraws the sidebar and nothing else does.
    sources: Vec<(TabId, usize, usize)>,
    _changed: Subscription,
}

impl Activity {
    pub(crate) fn new(cx: &mut Context<HerdrWindow>) -> Self {
        let filter = cx.new(SearchInput::new);
        let changed = cx.subscribe(&filter, |_, _, _: &Changed, cx| cx.notify());
        Self {
            panel: ActivityPanel::default(),
            picked: false,
            filter,
            sources: Vec::new(),
            _changed: changed,
        }
    }

    /// Whether a list panel's filter holds the keyboard.
    pub(crate) fn filter_focused(&self, window: &Window, cx: &App) -> bool {
        self.filter.read(cx).focus.contains_focused(window, cx)
    }

    /// The panel to store: only one the user picked.
    pub(crate) fn chosen(&self) -> Option<ActivityPanel> {
        self.picked.then_some(self.panel)
    }

    /// Applies the stored panel unless one was picked before it loaded.
    pub(crate) fn restore(&mut self, stored: Option<ActivityPanel>) {
        if let Some(panel) = stored.filter(|_| !self.picked) {
            self.panel = panel;
            self.picked = true;
        }
    }

    /// Records the views' lists; whether they changed since last time.
    pub(crate) fn observe_sources(
        &mut self,
        views: &[(TabId, Entity<OrchestratorView>)],
        cx: &App,
    ) -> bool {
        let mut sources: Vec<_> = views
            .iter()
            .map(|(id, view)| {
                let view = view.read(cx);
                (
                    *id,
                    Arc::as_ptr(view.items()) as usize,
                    Arc::as_ptr(view.runs()) as usize,
                )
            })
            .collect();
        sources.sort_unstable();
        if sources == self.sources {
            return false;
        }
        self.sources = sources;
        true
    }
}

impl HerdrWindow {
    /// Whether the expanded sidebar leads with the bar.
    pub(crate) fn activity_bar_shown(&self) -> bool {
        self.config.sidebar_style.activity_bar
    }

    /// The sidebar column's width as its mode lays it out, with the bar's
    /// room added beside an expanded panel.
    pub(crate) fn sidebar_column_width(&self, window_width: f32) -> Option<f32> {
        let mode = self.sidebar_mode();
        let width = mode.width(self.sidebar_width, window_width)?;
        let bar = mode == super::SidebarMode::Expanded && self.activity_bar_shown();
        Some(if bar { width + BAR_WIDTH } else { width })
    }

    /// Shows `panel`, expanding the sidebar for it. Picking the panel already
    /// shown collapses the sidebar instead, as a second click on a tab bar's
    /// selected icon does.
    pub(crate) fn select_activity_panel(&mut self, panel: ActivityPanel, cx: &mut Context<Self>) {
        if panel == self.activity.panel && self.sidebar_visible {
            self.toggle_sidebar();
        } else {
            self.show_activity_panel(panel, cx);
        }
        cx.notify();
    }

    /// Shows `panel` with the sidebar expanded, whatever was shown before.
    pub(crate) fn show_activity_panel(&mut self, panel: ActivityPanel, cx: &mut Context<Self>) {
        if !self.sidebar_visible {
            self.toggle_sidebar();
        }
        if panel != self.activity.panel {
            self.activity.panel = panel;
            self.activity.picked = true;
            self.activity.filter.update(cx, |input, cx| input.clear(cx));
            self.save_chrome();
        }
        cx.notify();
    }

    /// The bar and the panel it picked, in place of the plain sidebar.
    pub(crate) fn render_activity(
        &self,
        indicators: Indicators,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let panel = self.activity.panel;
        let lists = self.activity_lists(cx);
        let body = match panel {
            ActivityPanel::Workspaces => self
                .render_sidebar(indicators, window, cx)
                .into_any_element(),
            _ => self
                .render_list_panel(panel, &lists, window, cx)
                .into_any_element(),
        };
        div()
            .id("activity")
            .debug_selector(|| "activity".into())
            .flex()
            .flex_none()
            .h_full()
            .min_h_0()
            .child(self.render_activity_bar(&lists, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .h_full()
                    .min_h_0()
                    .child(self.panel_title(panel))
                    .child(div().flex().flex_1().min_h_0().child(body)),
            )
    }

    fn render_activity_bar(&self, lists: &panels::Lists, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let mut bar = div()
            .debug_selector(|| "activity-bar".into())
            .w(px(BAR_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .py(px(6.))
            .bg(self.fill().panel(theme.sidebar_background()))
            .border_r_1()
            .border_color(rgb(theme.active))
            // The overview page is planned; its place is kept, unclickable.
            .child(self.bar_button(
                "activity-overview",
                "icons/pulse.svg",
                "Overview (coming soon)",
                BarState::Disabled,
                None,
            ));
        for &panel in ActivityPanel::ALL {
            let state = if panel == self.activity.panel {
                BarState::Selected
            } else {
                BarState::Idle
            };
            let button = self
                .bar_button(
                    panel_selector(panel),
                    panel.icon(),
                    panel.label(),
                    state,
                    lists.count(panel),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_activity_panel(panel, cx);
                }));
            bar = bar.child(button);
        }
        bar.child(div().flex_1()).child(
            self.bar_button(
                "activity-settings",
                "icons/settings.svg",
                "Settings",
                BarState::Idle,
                None,
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.command(Command::Settings, window, cx);
            })),
        )
    }

    fn bar_button(
        &self,
        id: &'static str,
        icon: &'static str,
        label: &'static str,
        state: BarState,
        count: Option<usize>,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let (foreground, surface, hover) = (theme.foreground, theme.surface, theme.active);
        let ink = match state {
            BarState::Selected => theme.foreground,
            BarState::Idle => theme.muted,
            BarState::Disabled => theme.dimmed(theme.muted),
        };
        let glyph = div()
            .relative()
            .child(
                svg()
                    .path(icon)
                    .size(px(ICON))
                    .flex_none()
                    .text_color(rgb(ink)),
            )
            .when_some(count.filter(|count| *count > 0), |glyph, count| {
                glyph.child(
                    div()
                        .absolute()
                        .top(px(-8.))
                        .left(px(10.))
                        .child(self.count_bubble(count)),
                )
            });
        div()
            .id(id)
            .debug_selector(move || id.into())
            .relative()
            .size(px(BUTTON))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(crate::config::corners::CONTROL))
            .tooltip(move |_, cx| {
                cx.new(|_| crate::menu::SettingsHint {
                    text: label.into(),
                    foreground,
                    surface,
                })
                .into()
            })
            .when(state == BarState::Selected, |button| {
                button.bg(rgb(theme.primary_wash())).child(
                    div()
                        .absolute()
                        .left(px(-6.))
                        .top(px(6.))
                        .bottom(px(6.))
                        .w(px(2.))
                        .rounded_full()
                        .bg(rgb(theme.primary())),
                )
            })
            .when(state == BarState::Idle, |button| {
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(hover)))
            })
            .child(glyph)
    }

    fn count_bubble(&self, count: usize) -> Div {
        let theme = &self.theme;
        let text = if count > 99 {
            "99+".to_owned()
        } else {
            count.to_string()
        };
        div()
            .min_w(px(14.))
            .h(px(14.))
            .px(px(3.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(rgb(theme.sidebar_background()))
            .bg(rgb(theme.primary()))
            .text_color(rgb(theme.text_on(theme.primary())))
            .text_size(px(9.))
            .line_height(px(12.))
            .child(text)
    }

    /// The panel's name over its list, across the panel's width.
    fn panel_title(&self, panel: ActivityPanel) -> Div {
        let theme = &self.theme;
        let font = &self.config.sidebar;
        div()
            .debug_selector(|| "activity-title".into())
            .flex_none()
            .h(px(TITLE_HEIGHT.max(line_height(font) + 8.)))
            .px(px(12.))
            .flex()
            .items_center()
            .text_font(font)
            .text_size(px(font.size - 1.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme.subtext()))
            .bg(self.fill().panel(theme.sidebar_background()))
            .border_r_1()
            .border_color(rgb(theme.active))
            .child(panel.label().to_uppercase())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BarState {
    Selected,
    Idle,
    Disabled,
}

fn panel_selector(panel: ActivityPanel) -> &'static str {
    match panel {
        ActivityPanel::Workspaces => "activity-workspaces",
        ActivityPanel::Orchestrator => "activity-orchestrator",
        ActivityPanel::PullRequests => "activity-pull-requests",
        ActivityPanel::Issues => "activity-issues",
    }
}
