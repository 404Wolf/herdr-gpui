use super::*;

#[gpui::test]
fn a_caller_naming_its_daemon_never_reaches_another_sessions_workspace(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    cx.update(|_, cx| {
        let view = view.read(cx);
        // The window shows a w3, but on another daemon than the caller's:
        // a same-named workspace of another session is never acted on.
        let elsewhere = Caller {
            daemon_socket: Some("/tmp/another-session.sock".into()),
            ..caller(Some("w3"))
        };
        let refused = Place::of(view, &elsewhere).unwrap_err();
        assert_eq!(error_code(&refused), Some(ErrorCode::WorkspaceNotFound));
        // Without a daemon, any endpoint showing the workspace still serves.
        assert_eq!(
            Place::of(view, &caller(Some("w3"))).unwrap().workspace,
            "w3"
        );
    });
}

#[test]
fn a_page_request_succeeds_only_when_a_page_opened() {
    let done = || "Opened port 3000".to_owned();
    assert!(matches!(
        page_answer(Some(PageOpened::Tab), done),
        Response::Done { .. }
    ));
    assert!(matches!(
        page_answer(Some(PageOpened::SystemBrowser), done),
        Response::Done { .. }
    ));
    assert_eq!(
        error_code(&page_answer(Some(PageOpened::Full), done)),
        Some(ErrorCode::TabLimit)
    );
    assert_eq!(
        error_code(&page_answer(Some(PageOpened::Unreachable), done)),
        Some(ErrorCode::Failed)
    );
    let Response::Done { message } = page_answer(None, done) else {
        panic!("a tunnel still connecting is not a failure");
    };
    assert!(message.contains("once its tunnel connects"), "{message}");
}
