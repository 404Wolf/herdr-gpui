//! Linked worktree rows wear their hue, and the branch glyph when they stand
//! alone; the main checkout and plain folders keep the theme as drawn.
use super::*;
use crate::{config::LayoutMode, worktree_identity::Worktrees};

/// The fixture's repository: `agent-launcher` is the main checkout (w3) and
/// w5 is a linked worktree nested under it. w4 becomes a linked worktree of
/// a repository whose main checkout is not open, so it stands alone. `herdr`
/// is a folder outside Git.
const MAIN: &str = "agent-launcher";
const CHILD: &str = "sidebar-child-with-a-long-readable-branch-name";
const ALONE: &str = "launcher-hotfix";
const PLAIN: &str = "herdr";

/// Bounds of the element probed as `selector`. The probe API takes static
/// names, and these are built per row, so each one is leaked: a test's
/// handful of short strings.
fn probe(cx: &mut gpui::VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
}

fn draw(
    cx: &mut gpui::TestAppContext,
    mode: LayoutMode,
    worktrees: Worktrees,
    repository_icons: bool,
) -> &mut gpui::VisualTestContext {
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let mut view = fixture_window(window, cx);
            view.config.layout.mode = mode;
            view.config.worktrees = worktrees;
            view.config.show_repository_icons = repository_icons;
            let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
            let alone = &mut snapshot.workspaces[4];
            alone.label = ALONE.into();
            alone.worktree = Some(ClientShellWorktree {
                key: "/fixture/elsewhere/.git".into(),
                label: "elsewhere".into(),
                is_linked_worktree: true,
            });
            view
        });
        cx.observe(&view, |_, _, cx| cx.notify()).detach();
        SidebarFixture(view)
    });
    cx.simulate_resize(size(px(800.), px(900.)));
    cx.run_until_parked();
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    cx
}

/// Every layout washes the linked worktrees' rows, nested or alone, and
/// only theirs.
#[gpui::test]
fn every_layout_washes_only_linked_worktrees(cx: &mut gpui::TestAppContext) {
    for mode in LayoutMode::ALL
        .into_iter()
        .filter(|mode| mode.lists_workspaces())
    {
        let cx = draw(cx, mode, Worktrees::default(), true);
        for key in [CHILD, ALONE] {
            assert!(
                probe(cx, format!("worktree-wash-{key}")).is_some(),
                "{mode}: {key} unwashed"
            );
        }
        for key in [MAIN, PLAIN] {
            assert!(
                probe(cx, format!("worktree-wash-{key}")).is_none(),
                "{mode}: {key} washed"
            );
        }
    }
}

/// In Herdr's own rows a linked worktree standing alone takes the branch
/// glyph in the slot its owner's avatar used, so it never relies on colour
/// alone, even with the repository icons off. A nested child is marked by
/// its place under the main checkout and keeps its geometry.
#[gpui::test]
fn a_linked_worktree_standing_alone_leads_with_the_branch_glyph(cx: &mut gpui::TestAppContext) {
    for mode in [
        LayoutMode::from(crate::config::Density::Comfortable),
        LayoutMode::default(),
    ] {
        for icons in [true, false] {
            let cx = draw(cx, mode, Worktrees::default(), icons);
            let glyph = probe(cx, format!("worktree-icon-{ALONE}")).unwrap();
            let name = probe(cx, format!("name-{ALONE}")).unwrap();
            let row = probe(cx, format!("row-{ALONE}")).unwrap();
            assert!(glyph.right() <= name.left(), "{mode}");
            assert!(name.right() <= row.right(), "{mode}");
            assert_eq!(glyph.size, size(px(12.), px(12.)), "{mode}");
            assert!(probe(cx, format!("github-{ALONE}")).is_none());
            // The main checkout's avatar slot is where it was.
            let main = probe(cx, format!("github-{MAIN}"));
            assert_eq!(main.is_some(), icons, "{mode}");
            if let Some(main) = main {
                assert_eq!(main.left(), glyph.left(), "{mode}");
            }
            for key in [MAIN, CHILD, PLAIN] {
                assert!(
                    probe(cx, format!("worktree-icon-{key}")).is_none(),
                    "{mode}: {key}"
                );
            }
        }
    }
}

/// The wash lies inside its row, in the shape of the row's highlight.
#[gpui::test]
fn the_wash_lies_inside_its_row(cx: &mut gpui::TestAppContext) {
    for mode in [
        LayoutMode::default(),
        LayoutMode::new(
            crate::config::Density::Comfortable,
            crate::config::Style::Rounded,
        ),
    ] {
        let cx = draw(cx, mode, Worktrees::default(), true);
        for key in [CHILD, ALONE] {
            let wash = probe(cx, format!("worktree-wash-{key}")).unwrap();
            let row = probe(cx, format!("row-{key}")).unwrap();
            assert!(
                wash.left() >= row.left() && wash.right() <= row.right(),
                "{mode}"
            );
            assert!(
                wash.top() >= row.top() && wash.bottom() <= row.bottom(),
                "{mode}"
            );
            assert!(wash.size.height > row.size.height / 2., "{mode}");
        }
    }
}

/// `apply_to` without `sidebar` leaves every row as the theme draws it,
/// with the owner's avatar back on the row standing alone.
#[gpui::test]
fn turning_the_sidebar_off_leaves_rows_unmarked(cx: &mut gpui::TestAppContext) {
    let cx = draw(
        cx,
        LayoutMode::default(),
        Worktrees::new(None, &[crate::worktree_identity::Surface::Terminal]),
        true,
    );
    for key in [CHILD, ALONE] {
        assert!(probe(cx, format!("worktree-wash-{key}")).is_none());
        assert!(probe(cx, format!("worktree-icon-{key}")).is_none());
    }
    assert!(probe(cx, format!("github-{ALONE}")).is_some());
}
