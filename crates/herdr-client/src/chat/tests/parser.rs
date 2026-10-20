use super::*;

#[test]
fn pairs_tools_without_inventing_user_turns_and_ignores_context() {
    let result = parse(&jsonl(&[
        message("user", "1", None, json!([{"type":"text","text":"Fix it"}])),
        message("assistant", "2", Some("1"), json!([
            {"type":"text","text":"Checking."},
            {"type":"tool_use","id":"call","name":"Read","input":{"file":"src/main.rs"}}
        ])),
        message("user", "3", Some("2"), json!([{"type":"tool_result","tool_use_id":"call","content":[{"type":"text","text":"source"}]}])),
        message("assistant", "4", Some("3"), json!("Fixed.")),
        json!({"type":"user","uuid":"5","parentUuid":"4","isMeta":true,"message":{"content":"private context"}}),
    ]), ID).unwrap();
    assert_eq!(result.turns.len(), 2);
    assert_eq!(
        result.turns[1].parts[1],
        Part::Tool {
            id: "call".into(),
            name: "Read".into(),
            input: r#"{"file":"src/main.rs"}"#.into(),
            output: Some("source".into()),
            failed: false
        }
    );
    assert!(!format!("{result:?}").contains("private context"));
}

#[test]
fn follows_current_branch_and_replaces_duplicate_records() {
    let result = parse(
        &jsonl(&[
            message("user", "a", None, json!("start")),
            message("assistant", "b", Some("a"), json!("abandoned")),
            message("assistant", "c", Some("a"), json!("partial")),
            message("assistant", "c", Some("a"), json!("final")),
        ]),
        ID,
    )
    .unwrap();
    assert_eq!(result.turns[1].parts, vec![Part::Text("final".into())]);
}

#[test]
fn incomplete_utf8_is_deferred_but_complete_malformed_lines_fail() {
    let mut bytes = jsonl(&[message("user", "a", None, json!("hello"))]);
    bytes.extend_from_slice(b"{\"message\":\"\xf0\x9f");
    assert_eq!(parse(&bytes, ID).unwrap().turns.len(), 1);
    bytes.push(b'\n');
    assert!(matches!(parse(&bytes, ID), Err(Error::Record(_))));
}

#[test]
fn rejects_wrong_session_and_bounds_records_and_text() {
    let wrong =
        jsonl(&[json!({"type":"user","sessionId":"another","message":{"content":"wrong"}})]);
    assert!(matches!(parse(&wrong, ID), Err(Error::WrongSession)));
    assert!(matches!(
        parse(&vec![b'x'; MAX_LINE_BYTES + 1], ID),
        Err(Error::Limit)
    ));
    let text = "é".repeat(MAX_TEXT_BYTES);
    let result = parse(&jsonl(&[message("user", "a", None, json!(text))]), ID).unwrap();
    assert!(result.limited);
    let Part::Text(text) = &result.turns[0].parts[0] else {
        panic!("text")
    };
    assert!(text.len() < MAX_TEXT_BYTES + 100);
}

#[test]
fn compaction_is_an_explicit_history_boundary() {
    let result = parse(
        &jsonl(&[
            message("user", "a", None, json!("old")),
            json!({"type":"system","subtype":"compact_boundary","uuid":"b","parentUuid":"a"}),
            message("user", "c", Some("b"), json!("new")),
        ]),
        ID,
    )
    .unwrap();
    assert_eq!(result.boundary.as_deref(), Some("b"));
    assert_eq!(result.turns[0].parts, vec![Part::Text("new".into())]);
}
