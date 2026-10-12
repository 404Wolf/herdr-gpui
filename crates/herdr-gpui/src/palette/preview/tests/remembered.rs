use super::*;

/// What the daemon streams for the tab whose only pane is `pane`.
fn streaming(boot: &str, pane: &str) -> Arc<PaneSurfaceFrame> {
    let mut surface = Arc::unwrap_or_clone(surface(boot));
    surface.panes[0].pane_id = pane.into();
    Arc::new(surface)
}

/// `snapshot()` plus tabs `w1:t3`.. each holding one pane `w1:p3`...
fn many_tabs(count: usize) -> ClientShellSnapshot {
    let mut snapshot = snapshot();
    for n in 3..3 + count {
        let mut tab = snapshot.tabs[0].clone();
        tab.tab_id = format!("w1:t{n}");
        snapshot.tabs.push(tab);
        let mut pane = snapshot.panes[1].clone();
        pane.pane_id = format!("w1:p{n}");
        pane.tab_id = format!("w1:t{n}");
        snapshot.panes.push(pane);
    }
    snapshot
}

fn seen_at(preview: Option<Preview>) -> Option<Option<u64>> {
    match preview? {
        Preview::Screen { seen_at, .. } => Some(seen_at),
        Preview::Facts { .. } => None,
    }
}

#[test]
fn a_tab_left_behind_previews_its_last_frame_with_its_age() {
    let snapshot = snapshot();
    let agent = surface("boot-v1");
    let logs = streaming("boot-v1", "w1:p2");
    let mut seen = LastSeen::default();
    seen.observe(
        (Some(&snapshot), Some(&agent)),
        (Some(&snapshot), Some(&logs)),
        100,
    );
    let look =
        |seen: &LastSeen, target| preview(Some(&snapshot), Some(&logs), seen, "boot-v1", target);
    // The agent's tab is no longer streamed, but its last frame is on hand.
    assert_eq!(
        shown(look(&seen, NavigationTarget::Pane("w1:p1"))),
        Some(INNER)
    );
    assert_eq!(
        seen_at(look(&seen, NavigationTarget::Pane("w1:p1"))),
        Some(Some(100))
    );
    assert_eq!(
        seen_at(look(&seen, NavigationTarget::Tab("w1:t1"))),
        Some(Some(100))
    );
    // The streamed tab is live, never a remembered frame.
    assert_eq!(
        seen_at(look(&seen, NavigationTarget::Pane("w1:p2"))),
        Some(None)
    );
    // Going back forgets the frame: live cells take over.
    seen.observe(
        (Some(&snapshot), Some(&logs)),
        (Some(&snapshot), Some(&agent)),
        200,
    );
    assert_eq!(
        seen.frames("boot-v1").map(|(_, at)| at).collect::<Vec<_>>(),
        [200]
    );
    assert_eq!(
        seen_at(preview(
            Some(&snapshot),
            Some(&agent),
            &seen,
            "boot-v1",
            NavigationTarget::Pane("w1:p1")
        )),
        Some(None)
    );
}

#[test]
fn patches_within_one_tab_remember_nothing() {
    let snapshot = snapshot();
    let before = surface("boot-v1");
    let after = surface("boot-v1");
    let mut seen = LastSeen::default();
    seen.observe(
        (Some(&snapshot), Some(&before)),
        (Some(&snapshot), Some(&after)),
        100,
    );
    assert_eq!(seen.frames("boot-v1").count(), 0);
}

#[test]
fn a_new_daemon_boot_forgets_every_frame() {
    let snapshot = snapshot();
    let mut seen = LastSeen::default();
    seen.observe(
        (Some(&snapshot), Some(&surface("boot-v1"))),
        (Some(&snapshot), Some(&streaming("boot-v1", "w1:p2"))),
        100,
    );
    let mut restarted = snapshot.clone();
    restarted.boot_id = "boot-v2".into();
    seen.observe((Some(&snapshot), None), (Some(&restarted), None), 200);
    assert_eq!(seen.frames("boot-v1").count(), 0);
    assert_eq!(seen.frames("boot-v2").count(), 0);
}

#[test]
fn closed_panes_and_tabs_are_never_previewed_from_memory() {
    let snapshot = snapshot();
    let agent = surface("boot-v1");
    let mut seen = LastSeen::default();
    seen.observe(
        (Some(&snapshot), Some(&agent)),
        (Some(&snapshot), None),
        100,
    );
    let mut closed = snapshot.clone();
    closed.panes.retain(|pane| pane.pane_id != "w1:p1");
    assert!(
        preview(
            Some(&closed),
            None,
            &seen,
            "boot-v1",
            NavigationTarget::Pane("w1:p1")
        )
        .is_none()
    );
    // A tab already gone when it is left is not worth a slot.
    let mut gone = snapshot.clone();
    gone.tabs.retain(|tab| tab.tab_id != "w1:t1");
    let mut fresh = LastSeen::default();
    fresh.observe((Some(&snapshot), Some(&agent)), (Some(&gone), None), 100);
    assert_eq!(fresh.frames("boot-v1").count(), 0);
}

#[test]
fn a_host_remembers_a_bounded_number_of_tabs_newest_first() {
    let snapshot = many_tabs(20);
    let mut seen = LastSeen::default();
    for n in 3..23u64 {
        let pane = format!("w1:p{n}");
        seen.observe(
            (Some(&snapshot), Some(&streaming("boot-v1", &pane))),
            (Some(&snapshot), None),
            n,
        );
    }
    let ages: Vec<u64> = seen.frames("boot-v1").map(|(_, at)| at).collect();
    assert_eq!(ages, (7..23).rev().collect::<Vec<_>>());
}
