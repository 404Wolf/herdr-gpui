use super::*;

fn located(text: &str, column: u16) -> Option<(String, Option<u32>, Range<u16>)> {
    frame_link(&frame(text, 60, 1), column, 0, 0, 60).and_then(|link| match link.target {
        RowTarget::Path { path, line } => Some((path, line, link.columns)),
        RowTarget::Web(_) => None,
    })
}

#[test]
fn every_cell_of_a_pasted_path_links_to_the_original_name() {
    for name in [
        "/tmp/Shot 1 (it's).png",
        "/tmp/Shot, 1.png",
        "/tmp/Screenshot 2026-10-14 at 10.00.00\u{202f}AM.png",
        r"/tmp/a\b",
        "/tmp/[x]{y}<z>|w;v\"u`t",
        "/tmp/box\u{2500}name",
        "/tmp/wow!",
        "/tmp/$HOME~#^=*?&",
    ] {
        // The exact text a file drop pastes.
        let mut text = String::from("see ");
        escape_into(name, &mut text);
        let link = 4..text.chars().count() as u16;
        text.push_str(" now");
        let width = text.chars().count() as u16;
        let row = frame(&text, width, 1);
        let read = |column| frame_link(&row, column, 0, 0, width);
        for column in link.clone() {
            let found = read(column).unwrap();
            assert_eq!(
                found.target,
                RowTarget::Path {
                    path: name.into(),
                    line: None
                },
                "{text} at {column}"
            );
            assert_eq!(found.columns, link, "{text}");
        }
        assert!(read(link.end).is_none(), "{text}");
    }
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
