use super::*;
use herdr_client::chat::Mode;

#[gpui::test]
fn chat_requires_a_window_and_keeps_its_typed_wire_contract(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        assert_eq!(
            code(&ui(
                call(UiOp::Chat {
                    mode: Some(Mode::Chat)
                }),
                cx
            )),
            Some(ErrorCode::NoWindow)
        );
        let request = crate::control::protocol::Request::Ui(call(UiOp::Chat {
            mode: Some(Mode::Chat),
        }));
        let bytes = serde_json::to_vec(&request).unwrap();
        assert_eq!(
            serde_json::from_slice::<crate::control::protocol::Request>(&bytes).unwrap(),
            request
        );
        let reply = Reply::Chat {
            mode: Mode::Terminal,
            pane: "pane-1".into(),
        };
        assert_eq!(
            serde_json::from_value::<Reply>(serde_json::to_value(&reply).unwrap()).unwrap(),
            reply
        );
    });
}
