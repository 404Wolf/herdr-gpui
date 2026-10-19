//! Linked Git worktrees told apart from their repository's main checkout.
//!
//! The daemon already reports, per workspace, whether its checkout is a linked
//! worktree (its `.git` is a file pointing into another repository's
//! `worktrees/`), so nothing here reads Git. A linked worktree gets one hue: a
//! faint wash and the branch glyph on its sidebar row, and a hairline where
//! its tab strip meets its terminals. The main checkout keeps the theme as
//! drawn, so the odd one out is the copy, not the original.
//!
//! The hue is the theme's blue, as the notes kept on worktrees draw it, never
//! [`Theme::primary`]: that already means "selected", and a worktree row that
//! looked selected would say the wrong thing.

use crate::{
    config::Theme,
    contrast::{Contrast, distance, hue_shift},
};
use gpui::{Div, ParentElement, Styled, div, px, rgb};
use herdr_client::protocol::{ClientShellSnapshot, ClientShellWorkspace};
use serde::Deserialize;

#[cfg(test)]
mod tests;

/// Thickness of the rule along a linked worktree's tab strip, in logical
/// pixels: as thin as reads at a glance without becoming a border.
pub(crate) const RULE: f32 = 2.;

/// The palette entry the default hue comes from: the theme's blue.
const BLUE: usize = 4;

/// How far the wash turns the sidebar toward the hue, in OKLab chroma. It
/// keeps the sidebar's lightness, so labels keep their contrast over it.
const WASH_CHROMA: f32 = 0.15;

/// How different a washed row looks from a plain one, in OKLab distance:
/// just past the 0.02 a viewer notices side by side, so the wash reads at a
/// glance without competing with a selected row. The wash's alpha is solved
/// for this on each theme, since a light sidebar has far less room for
/// colour than a dark one.
const WASH_DISTANCE: f32 = 0.03;

/// The alpha range the wash may take: never so faint it vanishes, never so
/// strong that a selected or hovered row under it stops reading as one.
const WASH_ALPHA: (u32, u32) = (0x1a, 0x4d);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("worktrees.tint must be a #rgb or #rrggbb colour, not {0:?}")]
    InvalidTint(String),
}

/// Where a linked worktree's marks are drawn.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    /// The workspace's row in the sidebar.
    Sidebar,
    /// The edge between the workspace's tab strip and its terminals.
    Terminal,
}

/// `[worktrees]`, resolved: an optional hue and the surfaces that show it.
/// `apply_to = []` turns every mark off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Worktrees {
    /// A configured hue, as `0xRRGGBB`. Unset, the theme's blue.
    pub tint: Option<u32>,
    pub sidebar: bool,
    pub terminal: bool,
}

impl Default for Worktrees {
    fn default() -> Self {
        Self {
            tint: None,
            sidebar: true,
            terminal: true,
        }
    }
}

/// What a linked worktree's sidebar row is drawn with on one theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SidebarMarks {
    /// The branch glyph's colour.
    pub(crate) glyph: u32,
    /// The wash over the row, as a colour and an alpha. `None` under high
    /// contrast, which keeps every label at its full contrast; the glyph and
    /// the row's accessible name still mark the worktree.
    pub(crate) wash: Option<(u32, u32)>,
}

impl Worktrees {
    /// The marks for `surfaces` in `tint`, or the theme's hue without one.
    pub(crate) fn new(tint: Option<u32>, surfaces: &[Surface]) -> Self {
        Self {
            tint,
            sidebar: surfaces.contains(&Surface::Sidebar),
            terminal: surfaces.contains(&Surface::Terminal),
        }
    }

    /// The hue as a mark on `theme`'s chrome: the glyph and the rule. Its
    /// lightness moves only as far as the contrast setting needs to read on
    /// the background, the surface, a selected row, and Herdr's own sidebar
    /// colour when the theme sets one, so a configured colour keeps its hue.
    pub(crate) fn mark(&self, theme: &Theme) -> u32 {
        let mark = theme.ink(self.tint.unwrap_or(theme.palette[BLUE]));
        match theme.sidebar {
            Some(sidebar) => crate::contrast::ink_on_chrome(
                mark,
                [sidebar, theme.surface, theme.active],
                theme.contrast.mark_ratio(),
            ),
            None => mark,
        }
    }

    /// The sidebar's marks for linked worktrees on `theme`, unless the
    /// sidebar does not show them. Computed once per frame, not per row.
    pub(crate) fn sidebar_marks(&self, theme: &Theme) -> Option<SidebarMarks> {
        self.sidebar.then(|| {
            let glyph = self.mark(theme);
            let wash = (theme.contrast != Contrast::High).then(|| wash(theme, glyph));
            SidebarMarks { glyph, wash }
        })
    }

    /// The rule along the edge of a linked worktree's terminals, when they
    /// show one. Absolutely placed: the caller pins it to the edge, so the
    /// terminal keeps its size whichever workspace is shown.
    pub(crate) fn rule(&self, linked: bool, theme: &Theme) -> Option<Div> {
        (linked && self.terminal).then(|| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .h(px(RULE))
                .bg(rgb(self.mark(theme)))
        })
    }
}

/// The sidebar turned toward `hue` at its own lightness, and the least alpha
/// in [`WASH_ALPHA`] that puts a washed row [`WASH_DISTANCE`] from a plain one.
/// Blending happens in sRGB, which is far from linear in OKLab, so the alpha
/// is found by bisection rather than by proportion.
fn wash(theme: &Theme, hue: u32) -> (u32, u32) {
    let sidebar = theme.sidebar_background();
    let color = hue_shift(sidebar, hue, WASH_CHROMA);
    let (mut low, mut high) = WASH_ALPHA;
    if distance(over(sidebar, color, high), sidebar) <= WASH_DISTANCE {
        return (color, high);
    }
    while high - low > 1 {
        let middle = (low + high) / 2;
        if distance(over(sidebar, color, middle), sidebar) < WASH_DISTANCE {
            low = middle;
        } else {
            high = middle;
        }
    }
    (color, high)
}

/// `color` at `alpha` over `base`, per sRGB channel, as the wash layer is
/// composited.
fn over(base: u32, color: u32, alpha: u32) -> u32 {
    let channel = |shift: u32| {
        let (base, color) = ((base >> shift) & 255, (color >> shift) & 255);
        (base * (255 - alpha) + color * alpha + 127) / 255
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// Whether `workspace` is a linked worktree rather than a main checkout or a
/// folder outside Git, as the daemon reports it.
pub(crate) fn is_linked(workspace: &ClientShellWorkspace) -> bool {
    workspace
        .worktree
        .as_ref()
        .is_some_and(|tree| tree.is_linked_worktree)
}

/// Whether the workspace `snapshot` shows is a linked worktree.
pub(crate) fn focused_is_linked(snapshot: &ClientShellSnapshot) -> bool {
    snapshot
        .workspaces
        .iter()
        .find(|workspace| Some(&workspace.workspace_id) == snapshot.focused_workspace_id.as_ref())
        .is_some_and(is_linked)
}

/// Where a group's rule lies: along its strip's edge that meets the
/// terminals, inside the chrome so no cell is covered, or along the group's
/// top when it shows no strip. `below` is a strip under the terminals.
/// Returns the strip, and the rule when the group itself must carry it.
pub(crate) fn place_rule(
    strip: Option<Div>,
    rule: Option<Div>,
    below: bool,
) -> (Option<Div>, Option<Div>) {
    match (strip, rule) {
        (Some(strip), Some(rule)) => {
            let rule = if below { rule.top_0() } else { rule.bottom_0() };
            (Some(strip.child(rule)), None)
        }
        (strip, rule) => (strip, rule.map(Styled::top_0)),
    }
}
