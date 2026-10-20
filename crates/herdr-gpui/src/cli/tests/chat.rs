use super::*;
use control::{ControlCommand, UiCommand};
use herdr_client::chat::Mode;

#[test]
fn chat_modes_preserve_common_options_and_reject_invalid_values() {
    for (word, mode) in [
        ("on", Some(Mode::Chat)),
        ("off", Some(Mode::Terminal)),
        ("status", None),
    ] {
        let options =
            LaunchOptions::parse(["ui", "chat", word, "--json", "--window", "42"]).unwrap();
        let LaunchMode::Control(invocation) = options.mode else {
            panic!("control command")
        };
        assert_eq!(
            invocation.command,
            ControlCommand::Ui(UiCommand::Chat(mode))
        );
        assert!(invocation.json);
        assert_eq!(invocation.window, Some(42));
    }
    assert!(LaunchOptions::parse(["ui", "chat", "maybe"]).is_err());
    assert!(LaunchOptions::parse(["ui", "chat", "on", "extra"]).is_err());
}
