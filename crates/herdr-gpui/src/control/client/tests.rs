use super::*;

#[test]
fn output_is_whole_and_only_a_terminal_loses_controls() {
    let patch = format!("{}\r\n\u{1b}[31mred\u{1b}[0m\n", "+".repeat(2 << 20));
    let piped = for_terminal(&patch, false);
    assert_eq!(piped, patch, "a pipe gets the output unchanged and whole");
    let shown = for_terminal(&patch, true);
    assert_eq!(shown.chars().filter(|c| *c == '+').count(), 2 << 20);
    assert!(!shown.contains('\u{1b}') && !shown.contains('\r'));
    assert!(shown.ends_with("[31mred[0m\n"));
}

#[test]
fn every_error_code_has_one_exit_status() {
    let rejected = |code| crate::Error::ControlRejected {
        code,
        message: String::new(),
    };
    for (code, expected) in [
        (ErrorCode::InvalidUrl, exit::USAGE),
        (ErrorCode::NotFound, exit::NOT_FOUND),
        (ErrorCode::WorkspaceNotFound, exit::NOT_FOUND),
        (ErrorCode::NoWindow, exit::NOT_FOUND),
        (ErrorCode::Busy, exit::BUSY),
        (ErrorCode::WindowBusy, exit::BUSY),
        (ErrorCode::TabLimit, exit::BUSY),
        (ErrorCode::Timeout, exit::TIMEOUT),
        (ErrorCode::Unsupported, exit::UNSUPPORTED),
        (ErrorCode::InvalidRequest, exit::REFUSED),
        (ErrorCode::Failed, exit::REFUSED),
    ] {
        assert_eq!(status(&rejected(code)), expected, "{code:?}");
    }
    assert_eq!(status(&crate::Error::ControlUnsupported), exit::UNSUPPORTED);
}
