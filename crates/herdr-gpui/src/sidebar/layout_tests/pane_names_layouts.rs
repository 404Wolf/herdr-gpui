//! Pane names in every sidebar layout: many rows, blank names, repeated agent
//! names and truncation at narrow widths.
use super::pane_names::{mentions, modes, painted, prepare, show};
use super::*;

const SIDEBAR: f32 = 320.;

fn window(
    cx: &mut gpui::TestAppContext,
    snapshot: ClientShellSnapshot,
) -> (Entity<HerdrWindow>, &mut gpui::VisualTestContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.live.snapshot = Some(Arc::new(snapshot));
        view
    });
    cx.simulate_resize(size(px(800.), px(1400.)));
    cx.run_until_parked();
    (view, cx)
}

/// `count` agents in one workspace, each with its own pane label.
fn crowded(count: usize, label: impl Fn(usize) -> Option<String>) -> ClientShellSnapshot {
    let mut snapshot = snapshot(1);
    let template = snapshot.agents[0].clone();
    snapshot.agents = (0..count)
        .map(|index| {
            let mut agent = template.clone();
            agent.pane_id = format!("p{index}");
            agent
        })
        .collect();
    let labels: Vec<Option<String>> = (0..count).map(label).collect();
    let labels: Vec<Option<&str>> = labels.iter().map(|label| label.as_deref()).collect();
    prepare(&mut snapshot, &labels, &vec![None; count]);
    snapshot
}

fn row_bounds(cx: &mut gpui::VisualTestContext, index: usize) -> Option<Bounds<Pixels>> {
    // Debug selectors are static; these few leaked strings live for the test run.
    let column: &'static str = Box::leak(format!("column-agent-p{index}").into_boxed_str());
    let row: &'static str = Box::leak(format!("row-agent-p{index}").into_boxed_str());
    cx.debug_bounds(column).or_else(|| cx.debug_bounds(row))
}

#[gpui::test]
fn many_panes_each_keep_a_row_and_a_name_in_every_layout(cx: &mut gpui::TestAppContext) {
    let count = 14;
    let (view, cx) = window(cx, crowded(count, |index| Some(format!("pane-{index:02}"))));
    for mode in modes() {
        show(&view, mode, SIDEBAR, cx);
        let painted = painted(cx);
        let mut previous_bottom = None;
        for index in 0..count {
            assert!(
                mentions(&painted, &format!("pane-{index:02}")),
                "{mode:?} lost pane-{index:02}: {painted:?}"
            );
            let bounds = row_bounds(cx, index).unwrap_or_else(|| panic!("{mode:?} row {index}"));
            assert!(bounds.size.height > px(0.), "{mode:?} row {index}");
            assert!(
                bounds.size.width <= px(SIDEBAR) + px(1.),
                "{mode:?} row {index} {bounds:?} past the sidebar"
            );
            if let Some(bottom) = previous_bottom {
                assert!(
                    bounds.top() >= bottom - px(0.5),
                    "{mode:?} row {index} overlaps the row above"
                );
            }
            previous_bottom = Some(bounds.bottom());
        }
    }
}

#[gpui::test]
fn blank_and_whitespace_names_add_nothing_in_every_layout(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx, snapshot(1));
    let paint_with = |view: &Entity<HerdrWindow>,
                      cx: &mut gpui::VisualTestContext,
                      labels: [Option<&str>; 2],
                      titles: [Option<&str>; 2]| {
        view.update(cx, |view, cx| {
            let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
            prepare(snapshot, &labels, &titles);
            cx.notify();
        });
        modes()
            .into_iter()
            .map(|mode| {
                show(view, mode, SIDEBAR, cx);
                let mut painted = painted(cx);
                painted.sort();
                painted
            })
            .collect::<Vec<_>>()
    };
    let none = paint_with(&view, cx, [None, None], [None, None]);
    let blank = paint_with(&view, cx, [Some(""), Some("   \t ")], [Some(" "), Some("")]);
    // Blank names must paint exactly what no names paint: no empty label, no
    // extra separator.
    assert_eq!(blank, none);
    for mode in modes() {
        show(&view, mode, SIDEBAR, cx);
        let painted = painted(cx);
        assert!(
            row_bounds(cx, 0).is_some() && row_bounds(cx, 1).is_some(),
            "{mode:?}"
        );
        assert!(
            painted
                .iter()
                .all(|text| !text.trim().is_empty() || text == " \u{b7} "),
            "{mode:?}"
        );
    }
}

/// Every painted text with where it landed and what it rendered.
fn probes(
    cx: &mut gpui::VisualTestContext,
) -> std::collections::BTreeMap<String, (Bounds<Pixels>, String, Pixels)> {
    cx.update(|window, cx| {
        cx.default_global::<TextProbes>().0.clear();
        full_draw(window, cx).clear(cx);
        cx.global::<TextProbes>().0.clone()
    })
}

#[gpui::test]
fn a_name_that_repeats_the_agent_is_painted_once_in_every_layout(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx, snapshot(1));
    let paint_with = |view: &Entity<HerdrWindow>,
                      cx: &mut gpui::VisualTestContext,
                      labels: [Option<&str>; 2]| {
        view.update(cx, |view, cx| {
            let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
            prepare(snapshot, &labels, &[None, None]);
            cx.notify();
        });
        modes()
            .into_iter()
            .map(|mode| {
                show(view, mode, SIDEBAR, cx);
                let probes = probes(cx);
                let panes = [
                    cx.debug_bounds("pane-agent-p0"),
                    cx.debug_bounds("pane-agent-p1"),
                ];
                (mode, probes, panes)
            })
            .collect::<Vec<_>>()
    };
    let unnamed = paint_with(&view, cx, [None, None]);
    let repeated = paint_with(&view, cx, [Some("grok"), Some(" grok ")]);
    for ((mode, probes, panes), (_, baseline, _)) in repeated.iter().zip(&unnamed) {
        assert!(
            panes.iter().all(Option::is_none),
            "{mode:?} painted a pane label: {panes:?}"
        );
        assert!(probes.contains_key("grok"), "{mode:?} lost the agent name");
        // Painted text is keyed by string, so a second "grok" would hide behind
        // the agent name. Herdr has no pane label of its own: a pane segment on
        // its first line would add a separator and move the segments around it,
        // so the repeated name must paint exactly what no name paints.
        assert_eq!(probes, baseline, "{mode:?} painted the agent name again");
    }
}

/// Orca and Superset paint the pane as its own label ahead of the place, so a
/// long workspace label is cut before the name that tells two agents apart.
#[gpui::test]
fn a_long_workspace_cannot_cut_the_pane_name(cx: &mut gpui::TestAppContext) {
    let workspace = "w".repeat(60);
    let mut snapshot = crowded(2, |index| Some(["api", "web"][index].into()));
    snapshot.workspaces[0].label = workspace.clone();
    let (view, cx) = window(cx, snapshot);
    for mode in [
        crate::config::LayoutMode::Orca,
        crate::config::LayoutMode::Superset,
    ] {
        show(&view, mode, SIDEBAR, cx);
        let probes = probes(cx);
        for (index, pane) in ["api", "web"].into_iter().enumerate() {
            let (_, rendered, _) = probes
                .get(pane)
                .unwrap_or_else(|| panic!("{mode:?} painted no {pane}: {probes:?}"));
            assert_eq!(rendered, pane, "{mode:?} cut {pane}");
            let selector: &'static str = Box::leak(format!("pane-agent-p{index}").into_boxed_str());
            assert!(cx.debug_bounds(selector).is_some(), "{mode:?} {selector}");
        }
        // The place is still there, and it is what gave way.
        let place = format!("{workspace} \u{b7} tab 1");
        let (_, rendered, _) = probes
            .get(&place)
            .unwrap_or_else(|| panic!("{mode:?} painted no place: {probes:?}"));
        assert_ne!(rendered, &place, "{mode:?} left the place uncut");
    }
}

#[gpui::test]
fn long_names_truncate_inside_the_row_at_narrow_widths(cx: &mut gpui::TestAppContext) {
    let wide = "\u{65e5}\u{672c}\u{8a9e}".repeat(20);
    let long = "x".repeat(120);
    let (view, cx) = window(
        cx,
        crowded(3, |index| match index {
            0 => Some(long.clone()),
            1 => Some(wide.clone()),
            _ => Some("short".into()),
        }),
    );
    for mode in modes() {
        for width in [160., 200., 240., SIDEBAR] {
            show(&view, mode, width, cx);
            let probes = cx.update(|window, cx| {
                cx.default_global::<TextProbes>().0.clear();
                full_draw(window, cx).clear(cx);
                cx.global::<TextProbes>().0.clone()
            });
            assert!(
                probes.keys().any(|text| text.contains("short")),
                "{mode:?} {width}: the short name vanished"
            );
            for (index, needle) in [(0, long.as_str()), (1, wide.as_str())] {
                let edge = row_bounds(cx, index).unwrap_or_else(|| panic!("{mode:?} {width}"));
                assert!(
                    edge.size.width <= px(width) + px(1.),
                    "{mode:?} {width} row {index} {edge:?}"
                );
                let captions: Vec<_> = probes
                    .iter()
                    .filter(|(text, _)| text.contains(needle))
                    .collect();
                assert!(
                    !captions.is_empty(),
                    "{mode:?} {width}: row {index} painted no caption"
                );
                for (text, (bounds, rendered, glyphs)) in captions {
                    assert!(
                        bounds.right() <= edge.right() + px(0.5),
                        "{mode:?} {width} {bounds:?} past {edge:?}"
                    );
                    assert!(
                        *glyphs <= bounds.size.width + px(0.5),
                        "{mode:?} {width} {text}"
                    );
                    if rendered.chars().count() < text.chars().count() {
                        assert!(
                            rendered.ends_with('\u{2026}'),
                            "{mode:?} {width} {rendered}"
                        );
                    }
                }
            }
        }
    }
}

/// A long pane caption beside a long status must not take the agent's name:
/// Minimal gives the secondary labels only what the name leaves.
#[gpui::test]
fn minimal_keeps_the_agent_name_beside_a_long_caption_and_status(cx: &mut gpui::TestAppContext) {
    let caption = "c".repeat(100);
    let status = "s".repeat(60);
    let mut snapshot = crowded(1, |_| Some(caption.clone()));
    let agent = &mut snapshot.agents[0];
    let key = crate::sidebar::agents::status_text(agent.agent_status).to_owned();
    agent.state_labels.push((key, status.clone()));
    let (view, cx) = window(cx, snapshot);
    // Minimal shows the status text only when the agent rows ask for it.
    view.update(cx, |view, _| {
        view.config.sidebar_layout.agents.rows =
            vec![vec![crate::config::sidebar::ConfiguredToken::plain(
                crate::config::AgentToken::StateText,
            )]];
    });
    for width in [140., 160., 200., SIDEBAR] {
        show(&view, crate::config::LayoutMode::Minimal, width, cx);
        let probes = cx.update(|window, cx| {
            cx.default_global::<TextProbes>().0.clear();
            full_draw(window, cx).clear(cx);
            cx.global::<TextProbes>().0.clone()
        });
        let find = |needle: &str| probes.iter().find(|(text, _)| text.contains(needle));
        assert!(find(&caption).is_some(), "{width}: no caption");
        assert!(find(&status).is_some(), "{width}: no status text");
        let (_, (bounds, rendered, _)) =
            find("grok").unwrap_or_else(|| panic!("{width}: the agent name vanished"));
        assert!(bounds.size.width > px(0.), "{width}: name has no room");
        assert!(
            rendered.starts_with("grok"),
            "{width}: name was cut to {rendered:?}"
        );
    }
}
