use super::*;

fn pane(id: &str, label: Option<&str>) -> ClientShellPane {
    ClientShellPane {
        pane_id: id.to_owned(),
        workspace_id: "w0".to_owned(),
        tab_id: "t0".to_owned(),
        label: label.map(str::to_owned),
        cwd: None,
        foreground_cwd: None,
        focused: false,
        right_click_passthrough: false,
    }
}

/// Rename's label, then the title an unnamed pane already carries.
#[test]
fn pane_caption_prefers_a_rename_over_the_title() {
    let mut snapshot = layout_tests::snapshot(1);
    let agent = &mut snapshot.agents[0];
    agent.pane_id = "p0".into();
    agent.title = Some("reported".into());
    snapshot.panes = vec![pane("p0", Some("  renamed  "))];
    assert_eq!(
        pane_caption(&snapshot.agents[0], &snapshot),
        Some("renamed")
    );

    snapshot.panes[0].label = Some(" \t ".into());
    assert_eq!(
        pane_caption(&snapshot.agents[0], &snapshot),
        Some("reported")
    );

    snapshot.agents[0].title = Some("  ".into());
    snapshot.panes[0].label = None;
    assert_eq!(pane_caption(&snapshot.agents[0], &snapshot), None);

    snapshot.panes.clear();
    snapshot.agents[0].title = Some("from-title".into());
    assert_eq!(
        pane_caption(&snapshot.agents[0], &snapshot),
        Some("from-title")
    );

    snapshot.panes = vec![pane("other", Some("elsewhere"))];
    assert_eq!(
        pane_caption(&snapshot.agents[0], &snapshot),
        Some("from-title")
    );
}

/// Different pane names stay different on the row, including one that matches
/// the workspace. A name that only repeats the agent is left off.
#[test]
fn agent_rows_keep_a_pane_name_that_is_not_the_agent() {
    let place = Some(("herdr-gpui", None));
    let named = |pane| agent_labels("grok", place, None, pane);
    assert_ne!(named(Some("api")), named(Some("web")));
    assert_eq!(
        named(Some("api")),
        (vec![("herdr-gpui", true), ("api", false)], "grok")
    );
    assert_eq!(named(Some("grok")), named(None));
    assert_eq!(named(Some("  grok  ")), named(None));
    assert_ne!(named(Some("herdr-gpui")), named(None));
    assert_eq!(
        agent_labels("grok", None, None, Some("api")),
        (vec![("grok", true), ("api", false)], "")
    );
    assert_eq!(pane_beside("grok", Some("  api ")), Some("api"));
    assert_eq!(pane_beside("grok", Some("   ")), None);
}

/// Two agents of one name, state, workspace and tab sound the same unless
/// the spoken label carries the pane name.
#[test]
fn spoken_labels_tell_same_named_agents_apart_by_pane() {
    let mut snapshot = layout_tests::snapshot(2);
    for (agent, title) in snapshot.agents.iter_mut().zip(["api", "docs"]) {
        agent.workspace_id = "w0".into();
        agent.tab_id = "t0".into();
        agent.display_agent = Some("grok".into());
        agent.name = Some("grok".into());
        agent.title = Some(title.into());
    }
    let spoken = |snapshot: &ClientShellSnapshot, index: usize, nested| {
        crate::sidebar::agents::spoken_agent(&snapshot.agents[index], snapshot, nested, None)
    };
    for nested in [false, true] {
        assert!(
            spoken(&snapshot, 0, nested).contains("grok, api"),
            "{}",
            spoken(&snapshot, 0, nested)
        );
        assert!(
            spoken(&snapshot, 1, nested).contains("grok, docs"),
            "{}",
            spoken(&snapshot, 1, nested)
        );
        assert_ne!(spoken(&snapshot, 0, nested), spoken(&snapshot, 1, nested));
    }
    // A name that only repeats the agent adds nothing to say.
    snapshot.agents[0].title = Some("grok".into());
    assert!(!spoken(&snapshot, 0, false).contains("grok, grok"));
}
