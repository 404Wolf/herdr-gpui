#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn files_are_absolute_printable_paths_at_lines_from_one() {
    let target = editor_target("/src/main.rs", Some(42)).unwrap();
    assert_eq!(target.path, Path::new("/src/main.rs"));
    assert_eq!(target.line, Some(42));
    assert_eq!(editor_target("/src/main.rs", None).unwrap().line, None);
    for (path, line) in [
        ("", None),
        ("src/main.rs", None),
        ("/src/ma\u{1b}in.rs", None),
        ("/src/main.rs", Some(0)),
    ] {
        assert!(
            matches!(
                editor_target(path, line),
                Err(Response::Error {
                    code: ErrorCode::InvalidRequest,
                    ..
                })
            ),
            "{path:?} {line:?}"
        );
    }
    let long = format!("/{}", "a".repeat(MAX_PATH));
    assert!(editor_target(&long, None).is_err());
}
