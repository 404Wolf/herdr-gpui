#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn readers_get_what_came_after_their_sequence_number() {
    let mut log = Log::default();
    log.push(EventKind::TabOpened, "1", Some("w_1".into()));
    log.push(EventKind::Feedback, "w_1:p1", None);
    log.push(EventKind::TabClosed, "1", Some("w_1".into()));
    let all = log.after(0, &[]);
    assert_eq!(
        all.iter().map(|event| event.seq).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(log.after(2, &[]).len(), 1);
    let tabs = log.after(0, &[EventKind::TabOpened, EventKind::TabClosed]);
    assert_eq!(
        tabs.iter().map(|event| event.seq).collect::<Vec<_>>(),
        [1, 3]
    );
    assert!(log.after(3, &[]).is_empty());
}

#[test]
fn the_log_is_bounded_and_keeps_counting() {
    let mut log = Log::default();
    for index in 0..KEEP + 10 {
        log.push(EventKind::Note, index.to_string(), None);
    }
    assert_eq!(log.events.len(), KEEP);
    assert_eq!(log.seq, (KEEP + 10) as u64);
    // The oldest are gone; a reader that fell behind gets what is left.
    assert_eq!(log.after(0, &[]).first().unwrap().seq, 11);
    assert_eq!(log.after(0, &[]).len(), BATCH);
}

#[gpui::test]
fn changes_between_ticks_become_events(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        let mut log = Log::default();
        let before = Seen {
            windows: vec![1],
            tabs: [(5, "w_1".to_owned())].into(),
            hosts: [((1, "local".to_owned()), "online".to_owned())].into(),
            ..Seen::default()
        };
        let now = Seen {
            windows: vec![1, 2],
            active: Some(2),
            tabs: [(6, "w_2".to_owned())].into(),
            notes: 3,
            hosts: [((1, "local".to_owned()), "reconnecting".to_owned())].into(),
            ..Seen::default()
        };
        compare(&mut log, &before, &now, cx);
        let kinds: Vec<_> = log
            .after(0, &[])
            .into_iter()
            .map(|event| (event.kind, event.subject))
            .collect();
        assert_eq!(
            kinds,
            [
                (EventKind::WindowOpened, "2".to_owned()),
                (EventKind::WindowFocused, "2".to_owned()),
                (EventKind::TabOpened, "6".to_owned()),
                (EventKind::TabClosed, "5".to_owned()),
                (EventKind::Note, "3".to_owned()),
                (EventKind::Host, "local".to_owned()),
            ]
        );
    });
}
