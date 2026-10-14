use super::*;
use crate::sidebar::layout_tests::{fixture_window, full_draw};

#[gpui::test]
fn rendered_hover_resumes_typing_in_the_hovered_pane(cx: &mut gpui::TestAppContext) {
    let (endpoint, mut server) = connected_endpoint("ssh:hover-render");
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        prepare_mouse(&mut view, endpoint, cx);
        // Let the actual canvas measure its geometry without sending a resize
        // while this fixture prepares the matching daemon frame.
        view.last_queued_options = Some(view.options);
        view
    });
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    let (mut next, mut frame) = view.update(cx, |view, cx| {
        let mut frame = (**view.live.surface.as_ref().unwrap()).clone();
        let size = view.options.surface_size;
        assert!(size.cols > 4 && size.rows > 2);
        frame.frame.width = size.cols;
        frame.frame.height = size.rows;
        frame.surface_revision += 1;
        let split = size.cols / 2;
        frame.panes[0].rect = SurfaceRect {
            x: 0,
            y: 0,
            width: split,
            height: size.rows,
        };
        frame.panes[1].rect = SurfaceRect {
            x: split,
            y: 0,
            width: size.cols - split,
            height: size.rows,
        };
        for pane in &mut frame.panes {
            pane.inner_rect = pane.rect;
            pane.mouse_reporting = false;
        }
        view.live.surface = Some(Arc::new(frame.clone()));
        view.last_queued_options = Some(view.options);
        view.pending_resize = None;
        let next = (**view.live.snapshot.as_ref().unwrap()).clone();
        {
            let mut state = view.endpoints[1].connection.inbox.lock().unwrap();
            state.apply(ClientEvent::Snapshot(Arc::new(next.clone())));
            state.apply(ClientEvent::Surface(Arc::new(frame.clone())));
        }
        assert!(view.config.focus_follows_mouse);
        assert!(view.input_ready());
        cx.notify();
        (next, frame)
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| window.focus(&view.focus, cx));
        full_draw(window, cx).clear(cx);
    });
    let positions = view.read_with(cx, |view, _| {
        frame
            .panes
            .iter()
            .map(|pane| {
                view.bounds.origin
                    + point(
                        px(
                            (f32::from(pane.inner_rect.x) + f32::from(pane.inner_rect.width) / 2.)
                                * view.cell_width,
                        ),
                        px(f32::from(pane.inner_rect.height) / 2.
                            * view.config.terminal.line_height()),
                    )
            })
            .collect::<Vec<_>>()
    });
    cx.simulate_mouse_move(positions[0], None, gpui::Modifiers::default());
    cx.simulate_input("b");
    // The first draw may have measured a different surface than the fixture's
    // options, depending on the platform's font, and sent that resize before
    // the fixture reset them. It is not what this test is about.
    let mut first = server.receive();
    if matches!(first, ClientMessage::ClientShellResize { .. }) {
        first = server.receive();
    }
    assert_eq!(
        first,
        ClientMessage::ClientShellPaneInput {
            pane_id: "w1:p1".into(),
            events: vec![ClientPaneInputEvent::TextCommit("b".into())],
        }
    );

    // Use the hit-tested render listener, not focus_hovered_pane directly.
    cx.simulate_mouse_move(positions[1], None, gpui::Modifiers::default());
    view.read_with(cx, |view, _| assert!(!view.input_ready()));
    cx.simulate_input("d");
    let ClientMessage::ClientShellEndpointRequest { request, .. } = server.receive() else {
        panic!("missing hover focus request");
    };
    let request: serde_json::Value = serde_json::from_str(&request).unwrap();
    assert_eq!(request["method"], Method::PaneFocus.as_str());
    assert_eq!(request["params"], serde_json::json!({"pane_id": "w1:p2"}));
    server.respond(&request);
    let ClientMessage::ClientShellEndpointRequest { request, .. } = server.receive() else {
        panic!("missing ordered surface barrier");
    };
    let barrier: serde_json::Value = serde_json::from_str(&request).unwrap();
    assert_eq!(barrier["method"], Method::ClientShellSurfaceSet.as_str());
    next.revision += 1;
    next.focused_pane_id = Some("w1:p2".into());
    for pane in &mut next.panes {
        pane.focused = pane.pane_id == "w1:p2";
    }
    frame.projection_revision = next.revision;
    frame.surface_revision += 1;
    for pane in &mut frame.panes {
        pane.focused = pane.pane_id == "w1:p2";
    }
    view.update(cx, |view, cx| {
        {
            let mut state = view.endpoints[1].connection.inbox.lock().unwrap();
            state.apply(ClientEvent::Snapshot(Arc::new(next.clone())));
            state.apply(ClientEvent::Surface(Arc::new(frame)));
        }
        project_until(
            view,
            cx,
            "hovered pane frame before acknowledgement",
            |view| view.live.snapshot.as_ref().unwrap().revision == next.revision,
        );
        assert!(!view.input_ready());
    });
    // The real response travels through the socket worker and its inbox.
    let id = barrier["id"].as_str().unwrap();
    write_message(
        &mut server.stream,
        &ServerMessage::ClientShellEndpointResponseChunk {
            boot_id: next.boot_id.clone(),
            request_id: id.into(),
            final_chunk: true,
            data: serde_json::to_vec(&serde_json::json!({"id": id, "result": {
                "type": "client_shell_surface_set", "active": true,
                "projection_revision": next.revision,
            }}))
            .unwrap(),
        },
        MAX_GRAPHICS_FRAME_SIZE,
    )
    .unwrap();
    view.update(cx, |view, cx| {
        project_until(view, cx, "hover focus completion", HerdrWindow::input_ready);
        assert!(!view.live.activation_pending());
        assert!(view.activation_deadline.is_none());
    });
    cx.update(|window, cx| full_draw(window, cx).clear(cx));
    cx.simulate_input("a");
    assert_eq!(
        server.receive(),
        ClientMessage::ClientShellPaneInput {
            pane_id: "w1:p2".into(),
            events: vec![ClientPaneInputEvent::TextCommit("a".into())],
        }
    );
}
