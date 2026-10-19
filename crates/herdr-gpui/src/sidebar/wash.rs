//! A host's colour over its rows. The config file names a colour per host;
//! painted faintly behind the host's header, its workspace rows, and its
//! agents' rows, it lets a long list read by colour. Selecting a workspace
//! can raise its host's wash and quiet the other hosts, so the active
//! machine stands out from the ones it shares the sidebar with.
//!
//! A linked worktree's row adds a second wash, in the worktree hue,
//! over its host's, so the two compose: the host (and later a project) says
//! whose row it is, the worktree wash says which checkout.

use super::layout::SidebarLook;
use crate::config::{SelectMode, SidebarStyle, Theme};
use gpui::{Div, InteractiveElement, ParentElement, Styled, div, px, rgba};

/// Alpha of a host's colour over rows at rest: a tint, never a fill, so
/// `theme.foreground` text stays legible on it in any theme.
pub(super) const REST_ALPHA: u32 = 0x1a;
/// Alpha over the selected host's rows in the group modes.
pub(super) const SELECTED_ALPHA: u32 = 0x38;
/// Opacity of every row on a host that is not selected in `group-dim`.
const DIM_OPACITY: f32 = 0.55;

/// A colour and how strongly it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Wash {
    pub(super) color: u32,
    pub(super) alpha: u32,
}

/// What one host's rows get: a wash, a dimming, both, or neither, and for
/// a linked worktree's row, its own wash over them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct HostMark {
    pub(super) wash: Option<Wash>,
    pub(super) dim: bool,
    pub(super) worktree: Option<Wash>,
}

impl HostMark {
    /// The host's mark on one row, with a linked worktree's wash, as a
    /// colour and an alpha, when the row is one the sidebar marks.
    pub(super) fn with_worktree(self, worktree: Option<(u32, u32)>) -> Self {
        Self {
            worktree: worktree.map(|(color, alpha)| Wash { color, alpha }),
            ..self
        }
    }

    /// The opacity of the host's rows.
    pub(super) fn opacity(self) -> f32 {
        if self.dim { DIM_OPACITY } else { 1. }
    }

    /// The mark for the host named `label`, given whether it holds the
    /// selected workspace. In `row` mode only configured colours show. The
    /// group modes raise the selected host's wash, with the theme's active
    /// colour standing in when no colour was configured, so the mode works
    /// without one; `group-dim` also quiets every other host.
    pub(super) fn resolve(
        style: &SidebarStyle,
        label: &str,
        selected: bool,
        theme: &Theme,
    ) -> Self {
        let configured = style.hosts.get(label).copied();
        let rest = configured.map(|color| Wash {
            color,
            alpha: REST_ALPHA,
        });
        match style.select {
            SelectMode::Row => Self {
                wash: rest,
                ..Self::default()
            },
            SelectMode::Group | SelectMode::GroupDim if selected => Self {
                wash: Some(Wash {
                    color: configured.unwrap_or(theme.active),
                    alpha: SELECTED_ALPHA,
                }),
                ..Self::default()
            },
            SelectMode::Group => Self {
                wash: rest,
                ..Self::default()
            },
            SelectMode::GroupDim => Self {
                wash: rest,
                dim: true,
                ..Self::default()
            },
        }
    }

    /// The row's wash layer and dimming. The layer is absolutely positioned
    /// and inset like the highlight; [`SidebarLook::washed`] decides which
    /// of the two paints on top. The row's content paints over both.
    pub(super) fn apply<E: ParentElement + Styled>(
        &self,
        row: E,
        key: &str,
        look: &SidebarLook,
    ) -> E {
        let row = if self.dim {
            row.opacity(DIM_OPACITY)
        } else {
            row
        };
        let shape = (look.inset(), look.spacing() / 2., look.density.radius);
        self.layers(row, key, shape)
    }

    /// As [`Self::apply`] for a row that is already its own card, such as
    /// orca's: the wash fills the card inside its corners.
    pub(super) fn fill<E: ParentElement + Styled>(&self, card: E, key: &str, radius: f32) -> E {
        let card = if self.dim {
            card.opacity(DIM_OPACITY)
        } else {
            card
        };
        self.layers(card, key, (0., 0., radius))
    }

    /// The host's wash, then the worktree's over it, in one `shape`.
    fn layers<E: ParentElement>(&self, row: E, key: &str, shape: (f32, f32, f32)) -> E {
        let host = self.wash.map(|wash| layer(("wash", key), wash, shape));
        let worktree = self
            .worktree
            .map(|wash| layer(("worktree-wash", key), wash, shape));
        row.children(host).children(worktree)
    }
}

/// The wash as a layer `inset` from the row's sides, `edge` from its top and
/// bottom, with `radius` corners, probed as `{name}-{key}`.
fn layer((name, key): (&str, &str), wash: Wash, (inset, edge, radius): (f32, f32, f32)) -> Div {
    div()
        .debug_selector(|| format!("{name}-{key}"))
        .absolute()
        .left(px(inset))
        .right(px(inset))
        .top(px(edge))
        .bottom(px(edge))
        .rounded(px(radius))
        .bg(rgba((wash.color << 8) | wash.alpha))
}
