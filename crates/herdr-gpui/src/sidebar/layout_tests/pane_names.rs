use super::*;
use crate::config::{AgentsPlacement, Density, LayoutMode};
use crate::preferences::AgentSort;

const API: &str = "api";
const WEB: &str = "web";
const TITLE: &str = "from-title";
const LONG: &str = "api-service-with-a-deliberately-long-pane-name";

fn modes() -> impl IntoIterator<Item = LayoutMode> {
    [
        LayoutMode::from(Density::Comfortable),
        LayoutMode::Superset,
        LayoutMode::Orca,
        LayoutMode::Minimal,
    ]
}

/// Point both fixture agents at one workspace and one agent name, then give
/// each the pane label and title under test.
fn prepare(snapshot: &mut ClientShellSnapshot, labels: &[Option<&str>], titles: &[Option<&str>]) {
    for agent in &mut snapshot.agents {
        agent.workspace_id = "w0".into();
        agent.tab_id = "t0".into();
        agent.display_agent = Some("grok".into());
        agent.name = Some("grok".into());
        agent.agent = Some("grok".into());
        agent.title = None;
    }
    let ids: Vec<_> = snapshot
        .agents
        .iter()
        .map(|agent| agent.pane_id.clone())
        .collect();
    for (agent, title) in snapshot.agents.iter_mut().zip(titles) {
        agent.title = title.map(str::to_owned);
    }
    snapshot.panes = ids
        .into_iter()
        .zip(labels)
        .map(|(pane_id, label)| ClientShellPane {
            pane_id,
            workspace_id: "w0".into(),
            tab_id: "t0".into(),
            label: label.map(str::to_owned),
            cwd: None,
            foreground_cwd: None,
            focused: false,
            right_click_passthrough: false,
        })
        .collect();
}

fn show(
    view: &Entity<HerdrWindow>,
    mode: LayoutMode,
    width: f32,
    cx: &mut gpui::VisualTestContext,
) {
    view.update(cx, |view, cx| {
        view.config.layout.mode = mode;
        view.config.show_agents = true;
        view.config.agents_placement = AgentsPlacement::Panel;
        view.config.usage.inline = false;
        view.agent_sort = AgentSort::Grouped;
        view.sidebar_width = Some(width);
        cx.notify();
    });
}

fn painted(cx: &mut gpui::VisualTestContext) -> Vec<String> {
    cx.update(|window, cx| {
        cx.default_global::<TextProbes>().0.clear();
        full_draw(window, cx).clear(cx);
        cx.global::<TextProbes>().0.keys().cloned().collect()
    })
}

fn mentions(painted: &[String], text: &str) -> bool {
    painted.iter().any(|label| label.contains(text))
}

#[gpui::test]
fn grouped_agent_rows_show_each_panes_name(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        let mut snapshot = snapshot(1);
        prepare(
            &mut snapshot,
            &[Some(API), Some(WEB)],
            &[Some(TITLE), Some(TITLE)],
        );
        view.live.snapshot = Some(Arc::new(snapshot));
        view
    });
    cx.simulate_resize(size(px(800.), px(900.)));
    cx.run_until_parked();

    for mode in modes() {
        show(&view, mode, 320., cx);
        let painted = painted(cx);
        assert!(
            mentions(&painted, API) && mentions(&painted, WEB),
            "{mode:?} should name both panes: {painted:?}"
        );
        assert!(
            !mentions(&painted, TITLE),
            "{mode:?} should prefer the rename over {TITLE}: {painted:?}"
        );
    }

    view.update(cx, |view, cx| {
        let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
        prepare(
            snapshot,
            &[None, None],
            &[Some("alpha-title"), Some("beta-title")],
        );
        cx.notify();
    });
    for mode in modes() {
        show(&view, mode, 320., cx);
        let painted = painted(cx);
        assert!(
            mentions(&painted, "alpha-title") && mentions(&painted, "beta-title"),
            "{mode:?} should use an unnamed pane's title: {painted:?}"
        );
    }

    view.update(cx, |view, cx| {
        let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
        prepare(snapshot, &[None, None], &[None, None]);
        cx.notify();
    });
    for mode in modes() {
        show(&view, mode, 320., cx);
        let painted = painted(cx);
        assert!(cx.debug_bounds("row-agent-p0").is_some(), "{mode:?}");
        assert!(cx.debug_bounds("row-agent-p1").is_some(), "{mode:?}");
        assert!(
            !mentions(&painted, API)
                && !mentions(&painted, WEB)
                && !mentions(&painted, TITLE)
                && !mentions(&painted, "alpha-title")
                && !mentions(&painted, "beta-title")
                && !mentions(&painted, "pane"),
            "{mode:?} should add nothing when the pane is unnamed: {painted:?}"
        );
    }

    view.update(cx, |view, cx| {
        let snapshot = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
        prepare(snapshot, &[Some(LONG), Some(WEB)], &[None, None]);
        cx.notify();
    });
    for mode in modes() {
        show(&view, mode, 160., cx);
        let probes = cx.update(|window, cx| {
            cx.default_global::<TextProbes>().0.clear();
            full_draw(window, cx).clear(cx);
            cx.global::<TextProbes>().0.clone()
        });
        let edge = cx
            .debug_bounds("column-agent-p0")
            .or_else(|| cx.debug_bounds("row-agent-p0"))
            .unwrap();
        let hits: Vec<_> = probes
            .iter()
            .filter(|(text, _)| text.contains(LONG))
            .collect();
        assert!(!hits.is_empty(), "{mode:?}");
        assert!(
            probes.keys().any(|text| text.contains(WEB)),
            "{mode:?} should still name the other pane"
        );
        for (text, (bounds, rendered, glyphs)) in hits {
            assert!(
                bounds.right() <= edge.right(),
                "{mode:?} {text} {bounds:?} past {edge:?}"
            );
            assert!(*glyphs <= bounds.size.width, "{mode:?} {text}");
            if rendered.chars().count() < text.chars().count() {
                assert!(rendered.ends_with('…'), "{mode:?} {rendered}");
            }
        }
    }
}
