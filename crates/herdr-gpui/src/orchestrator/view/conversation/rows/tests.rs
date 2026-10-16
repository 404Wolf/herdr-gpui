use super::*;
use gpui::ListOffset;

fn comment(author: &str, kind: CommentKind, at: &str, source: &str) -> Comment {
    Comment {
        author: author.into(),
        kind,
        body: String::new(),
        created_at: format!("2026-09-20T12:00:{at}Z"),
        source: source.into(),
        avatar: None,
        bot: false,
    }
}

fn thread(author: &str, resolved: bool, at: &str) -> Comment {
    let kind = CommentKind::Thread {
        path: "src/a.rs".into(),
        resolved,
        replies: 0,
    };
    comment(author, kind, at, "first\nsecond")
}

fn pieces(rows: &Rows) -> Vec<&Piece> {
    (0..rows.list.item_count())
        .map(|index| &rows.get(index).unwrap().piece)
        .collect()
}

#[test]
fn each_body_line_is_a_row_of_one_card() {
    let comments = [comment(
        "alice",
        CommentKind::Conversation,
        "01",
        "one\ntwo\nthree",
    )];
    let mut rows = Rows::default();
    rows.update(Some(&comments), Chrome::default());
    let cards: Vec<_> = (1..rows.list.item_count())
        .map(|index| {
            let row = rows.get(index).unwrap();
            (row.gutter, row.card.map(|card| (card.top, card.bottom)))
        })
        .collect();
    assert_eq!(
        cards,
        [
            (Gutter::Avatar, Some((true, false))),
            (Gutter::Blank, Some((false, false))),
            (Gutter::Blank, Some((false, false))),
            (Gutter::Blank, Some((false, true))),
        ]
    );
    assert_eq!(rows.list.item_count(), rows.rows.len());
}

#[test]
fn a_review_body_is_a_card_tinted_by_its_verdict() {
    let review = CommentKind::Review(Review::ChangesRequested);
    let comments = [comment("bob", review, "01", "fix this")];
    let mut rows = Rows::default();
    rows.update(Some(&comments), Chrome::default());
    let body = rows.get(2).unwrap();
    assert!(matches!(body.piece, Piece::Body { line: 0, .. }));
    assert_eq!(
        body.card,
        Some(Card {
            tint: Some(RED),
            top: true,
            bottom: true,
        })
    );
}

#[test]
fn a_reread_that_changes_nothing_keeps_the_scroll_position() {
    let comments = [
        comment("alice", CommentKind::Conversation, "01", "a\nb\nc"),
        comment("bob", CommentKind::Conversation, "02", "d\ne\nf"),
    ];
    let mut rows = Rows::default();
    rows.update(Some(&comments), Chrome::default());
    let top = ListOffset {
        item_ix: 6,
        offset_in_item: px(3.),
    };
    rows.list.scroll_to(top);
    rows.update(Some(&comments.clone()), Chrome::default());
    let kept = rows.list.logical_scroll_top();
    assert_eq!((kept.item_ix, kept.offset_in_item), (6, px(3.)));
}

#[test]
fn a_new_comment_below_keeps_the_rows_above_and_the_scroll_position() {
    let first = comment("alice", CommentKind::Conversation, "01", "a\nb\nc");
    let mut rows = Rows::default();
    rows.update(Some(std::slice::from_ref(&first)), Chrome::default());
    let before = rows.rows.clone();
    let top = ListOffset {
        item_ix: 2,
        offset_in_item: px(5.),
    };
    rows.list.scroll_to(top);
    let newer = comment("bob", CommentKind::Conversation, "02", "d");
    let comments = [first, newer];
    rows.update(Some(&comments), Chrome::default());
    let kept = rows.list.logical_scroll_top();
    assert_eq!((kept.item_ix, kept.offset_in_item), (2, px(5.)));
    // The summary counts one more comment; the first comment's rows stay.
    assert_eq!(rows.rows[1..before.len()], before[1..]);
    assert_eq!(rows.list.item_count(), rows.rows.len());
}

#[test]
fn unfolding_a_resolved_thread_replaces_only_its_row() {
    let comments = [
        comment("bob", CommentKind::Review(Review::Commented), "05", ""),
        thread("bob", true, "02"),
        comment("carol", CommentKind::Conversation, "06", "later"),
    ];
    let mut rows = Rows::default();
    rows.update(Some(&comments), Chrome::default());
    let folded = pieces(&rows)
        .iter()
        .position(|piece| matches!(piece, Piece::Folded { .. }))
        .unwrap();
    let Piece::Folded { key, .. } = &rows.rows[folded].piece else {
        unreachable!()
    };
    let (before, key) = (rows.rows.clone(), key.clone());
    rows.show(key);
    rows.update(Some(&comments), Chrome::default());
    assert_eq!(rows.rows[..folded], before[..folded]);
    assert!(matches!(rows.rows[folded].piece, Piece::Thread(_)));
    assert!(matches!(
        rows.rows[folded + 2].piece,
        Piece::Body { line: 1, .. }
    ));
    assert_eq!(rows.rows[folded + 3..], before[folded + 1..]);
    assert_eq!(rows.list.item_count(), before.len() + 2);
}

#[test]
fn the_chrome_frames_the_comments() {
    let comments = [comment("alice", CommentKind::Conversation, "01", "")];
    let chrome = Chrome {
        notice: None,
        outcome: Some("Commented".into()),
        failure: Some("Rate limited".into()),
        composer: true,
    };
    let mut rows = Rows::default();
    rows.update(Some(&comments), chrome);
    let drawn = pieces(&rows);
    assert!(matches!(drawn[0], Piece::Summary(_)));
    assert!(matches!(drawn[2], Piece::Empty));
    assert_eq!(
        drawn[3..],
        [
            &Piece::Notice("Commented".into()),
            &Piece::Failure("Rate limited".into()),
            &Piece::Composer,
        ]
    );
    assert_eq!(rows.rows[0].gap, 0.);
    // Signed out, only the reason shows.
    let signed_out = Chrome {
        notice: Some("Sign in".into()),
        ..Chrome::default()
    };
    rows.update(None, signed_out);
    assert_eq!(pieces(&rows), [&Piece::Notice("Sign in".into())]);
}
