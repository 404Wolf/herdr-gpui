use super::*;

fn located(text: &str, column: u16) -> Option<(String, Option<u32>, Range<u16>)> {
    frame_link(&frame(text, 60, 1), column, 0, 0, 60).and_then(|link| match link.target {
        RowTarget::Path { path, line } => Some((path, line, link.columns)),
        RowTarget::Web(_) => None,
    })
}

#[test]
fn a_dropped_path_links_to_the_original_name_from_any_of_its_cells() {
    // What a file drop pastes for `/tmp/Shot 1 (it's).png`.
    let text = r"see /tmp/Shot\ 1\ \(it\'s\).png now";
    let link = text.find("/tmp").unwrap() as u16..text.find(" now").unwrap() as u16;
    for column in [link.start, link.start + 9, link.end - 1] {
        assert_eq!(
            located(text, column),
            Some(("/tmp/Shot 1 (it's).png".into(), None, link.clone()))
        );
    }
    assert_eq!(located(text, link.end), None);
}

#[test]
fn backslashes_unescape_only_before_shell_special_characters() {
    for (text, path) in [
        (r"/tmp/a\\b", r"/tmp/a\b"),
        (r"/tmp/a\b", r"/tmp/a\b"),
        (r"/tmp/wow\!", "/tmp/wow!"),
        (r"/tmp/x\ y:12.", "/tmp/x y"),
    ] {
        assert_eq!(
            located(text, 1).map(|(path, _, _)| path),
            Some(path.into()),
            "{text}"
        );
    }
    assert_eq!(located(r"/tmp/x\ y:12.", 1).unwrap().1, Some(12));
}
