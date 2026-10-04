use super::*;

fn matches(rows: &[(u32, u16, u16)], current: Option<u32>, global: u64) -> CopySearchResult {
    CopySearchResult {
        pane_id: "p".into(),
        content_revision: 4,
        matches: rows
            .iter()
            .map(|&(row, start, end)| TextRange {
                start: point(row, start),
                end: point(row, end),
            })
            .collect(),
        total: 9,
        current,
        current_global: current.map(|_| global),
    }
}

/// Runs `command`, expecting a search, and records it as sent.
fn search(
    mode: &mut CopyMode,
    command: Command,
    shown: &PaneSurfacePane,
    id: &str,
) -> (CopySearchParams, bool) {
    let Outcome::Send(request) = mode.command(command, shown) else {
        panic!("a search asks the daemon");
    };
    mode.sent(id.into(), &request);
    let Request::Search { params, repeat } = request else {
        panic!("not a search: {request:?}");
    };
    (params, repeat)
}

fn found(mode: &mut CopyMode, id: &str, result: CopySearchResult) -> bool {
    mode.answer(id, Ok(ScrollbackResponse::PaneCopySearch(result)))
        .unwrap()
}

#[test]
fn search_keys_parse_as_herdr_copy_mode_defines_them() {
    use SearchDirection::*;
    let key = |key, shift| Command::from_key(key, shift, false);
    assert_eq!(key("/", false), Some(Command::Prompt(Forward)));
    // Shift-/ arrives as `/` with shift on some layouts and `?` on others.
    assert_eq!(key("/", true), Some(Command::Prompt(Backward)));
    assert_eq!(key("?", true), Some(Command::Prompt(Backward)));
    assert_eq!(key("n", false), Some(Command::Repeat { reverse: false }));
    assert_eq!(key("n", true), Some(Command::Repeat { reverse: true }));
    assert_eq!(Command::from_key("n", false, true), None);
    assert_eq!(Command::from_char('/'), Some(Command::Prompt(Forward)));
    assert_eq!(Command::from_char('?'), Some(Command::Prompt(Backward)));
    assert_eq!(
        Command::from_char('N'),
        Some(Command::Repeat { reverse: true })
    );
}

#[test]
fn the_prompt_opens_at_once_and_its_search_waits_its_turn() {
    let shown = pane(0);
    let mut mode = CopyMode::new(&shown, None);
    let request = mode.request(&Operation::Motion(CopyMotion::NextWordEnd), 4);
    mode.sent("1".into(), &request);
    // The prompt opens while the motion is out, so typing goes into it.
    assert_eq!(
        mode.command(Command::Prompt(SearchDirection::Forward), &shown),
        Outcome::Prompt(SearchDirection::Forward)
    );
    assert!(mode.queued.is_empty());
    // The submitted search runs after the motion lands, from its cursor.
    let submitted = Command::Search {
        query: "Needle".into(),
        direction: SearchDirection::Forward,
    };
    assert_eq!(mode.command(submitted.clone(), &shown), Outcome::Nothing);
    let landed = CopyMotionResult {
        pane_id: "p".into(),
        cursor: point(59, 7),
        content_revision: 4,
    };
    mode.answer("1", Ok(ScrollbackResponse::PaneCopyMotion(landed)))
        .unwrap();
    assert_eq!(mode.next_queued(), Some(submitted.clone()));
    let (params, repeat) = search(&mut mode, submitted, &shown, "2");
    assert!(!repeat);
    assert_eq!(
        params,
        CopySearchParams {
            pane_id: "p".into(),
            // Sent as typed: the daemon ignores case unless the query
            // has an uppercase letter, as Herdr's copy mode does.
            query: "Needle".into(),
            direction: SearchDirection::Forward,
            cursor: point(59, 7),
            content_revision: 4,
            previous: None,
        }
    );
    // An empty query searches for nothing.
    mode.in_flight = None;
    let empty = Command::Search {
        query: String::new(),
        direction: SearchDirection::Backward,
    };
    assert_eq!(mode.command(empty, &shown), Outcome::Nothing);
    assert_eq!(
        mode.command(Command::Repeat { reverse: false }, &shown),
        Outcome::Nothing,
        "n before any search"
    );
}

#[test]
fn a_match_moves_the_cursor_and_n_repeats_the_way_it_was_entered() {
    let shown = pane(0);
    let mut mode = CopyMode::new(&shown, None);
    let backward = Command::Search {
        query: "err".into(),
        direction: SearchDirection::Backward,
    };
    search(&mut mode, backward, &shown, "1");
    assert!(found(
        &mut mode,
        "1",
        matches(&[(40, 2, 4), (52, 6, 8)], Some(1), 4)
    ));
    assert_eq!(mode.cursor, point(52, 6));
    assert_eq!(mode.search_label().as_deref(), Some("?err 5 of 9"));
    // Matches on screen (rows 50..60) are tinted, the current one more.
    let highlights = mode.highlights(&shown);
    assert_eq!(highlights[0].tint, Tint::CurrentMatch);
    assert_eq!(highlights[0].row, 3);
    assert_eq!(highlights[0].columns, 8..11);
    assert_eq!(highlights.last().unwrap().tint, Tint::CopyCursor);

    // `n` goes on backward, past the match the cursor sits on.
    let (params, repeat) = search(&mut mode, Command::Repeat { reverse: false }, &shown, "2");
    assert!(repeat);
    assert_eq!(params.direction, SearchDirection::Backward);
    assert_eq!(params.query, "err");
    assert_eq!(
        params.previous,
        Some(TextRange {
            start: point(52, 6),
            end: point(52, 8),
        })
    );
    assert!(found(
        &mut mode,
        "2",
        matches(&[(40, 2, 4), (52, 6, 8)], Some(0), 3)
    ));
    assert_eq!(mode.cursor, point(40, 2));

    // `N` goes the other way but does not turn `n` around.
    let (params, _) = search(&mut mode, Command::Repeat { reverse: true }, &shown, "3");
    assert_eq!(params.direction, SearchDirection::Forward);
    assert!(found(
        &mut mode,
        "3",
        matches(&[(40, 2, 4), (52, 6, 8)], Some(1), 4)
    ));
    assert_eq!(mode.search_label().as_deref(), Some("?err 5 of 9"));
    // Off the match, a repeat searches from the cursor itself.
    mode.command(Command::Step { rows: 0, cols: 1 }, &shown);
    let (params, _) = search(&mut mode, Command::Repeat { reverse: false }, &shown, "4");
    assert_eq!(params.direction, SearchDirection::Backward);
    assert_eq!(params.previous, None);
    assert_eq!(params.cursor, point(52, 7));
}

#[test]
fn search_answers_for_a_moved_cursor_or_of_the_wrong_kind_are_dropped() {
    let shown = pane(0);
    let mut mode = CopyMode::new(&shown, None);
    let forward = Command::Search {
        query: "x".into(),
        direction: SearchDirection::Forward,
    };
    search(&mut mode, forward.clone(), &shown, "1");
    mode.cursor = point(3, 3);
    assert!(!found(&mut mode, "1", matches(&[(10, 0, 0)], Some(0), 0)));
    assert_eq!(mode.cursor, point(3, 3));
    assert_eq!(mode.search_label(), None);

    search(&mut mode, forward.clone(), &shown, "2");
    assert!(!found(&mut mode, "2", matches(&[], None, 0)));
    assert_eq!(mode.cursor, point(3, 3), "no match leaves the cursor");
    assert_eq!(mode.search_label().as_deref(), Some("/x 9 found"));

    search(&mut mode, forward, &shown, "3");
    let motion = CopyMotionResult {
        pane_id: "p".into(),
        cursor: point(1, 1),
        content_revision: 4,
    };
    assert!(matches!(
        mode.answer("3", Ok(ScrollbackResponse::PaneCopyMotion(motion))),
        Err(herdr_client::Error::ResponseType)
    ));
    assert_eq!(mode.cursor, point(3, 3));
}

#[test]
fn a_stale_search_retries_as_the_same_search() {
    let mut shown = pane(0);
    let mut mode = CopyMode::new(&shown, None);
    let backward = Command::Search {
        query: "x".into(),
        direction: SearchDirection::Backward,
    };
    search(&mut mode, backward, &shown, "1");
    found(&mut mode, "1", matches(&[(55, 1, 1)], Some(0), 0));
    search(&mut mode, Command::Repeat { reverse: true }, &shown, "2");
    let stale = herdr_client::Error::Endpoint {
        code: EndpointErrorCode::StaleContent,
        message: "pane content changed".into(),
    };
    assert!(!mode.answer("2", Err(stale)).unwrap());
    shown.content_revision = 6;
    let Some(Request::Search { params, repeat }) = mode.due_retry(&shown) else {
        panic!("the search is retried");
    };
    assert!(repeat);
    assert_eq!(params.direction, SearchDirection::Forward);
    assert_eq!(params.content_revision, 6);
    assert_eq!(params.previous.map(|range| range.start), Some(point(55, 1)));
}

#[test]
fn yank_copies_the_current_match_and_escape_clears_the_search_first() {
    let shown = pane(0);
    let mut mode = CopyMode::new(&shown, None);
    let forward = Command::Search {
        query: "x".into(),
        direction: SearchDirection::Forward,
    };
    search(&mut mode, forward, &shown, "1");
    found(&mut mode, "1", matches(&[(55, 1, 4)], Some(0), 0));
    assert_eq!(
        mode.command(Command::Copy, &shown),
        Outcome::Copy(TextRange {
            start: point(55, 1),
            end: point(55, 4),
        })
    );
    // A selection, marked from the match, wins over the match.
    mode.command(Command::Mark { lines: true }, &shown);
    assert_eq!(
        mode.command(Command::Copy, &shown),
        Outcome::Copy(TextRange {
            start: point(55, 0),
            end: point(55, 19),
        })
    );
    assert_eq!(mode.command(Command::Cancel, &shown), Outcome::Moved);
    assert_eq!(mode.search_label(), None);
    assert_eq!(mode.highlights(&shown).len(), 1, "only the cursor");
    assert_eq!(mode.command(Command::Copy, &shown), Outcome::Exit);
    assert_eq!(mode.command(Command::Cancel, &shown), Outcome::Exit);
}
