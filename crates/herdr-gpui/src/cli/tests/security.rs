use super::*;
use control::{ControlCommand, Invocation};

#[test]
fn security_takes_the_shared_options() {
    let parsed = LaunchOptions::parse(["security", "--json", "--window", "3"]).unwrap();
    let LaunchMode::Control(invocation) = parsed.mode else {
        panic!("security parsed as {:?}", parsed.mode);
    };
    assert_eq!(
        invocation,
        Invocation {
            command: ControlCommand::Security,
            json: true,
            window: Some(3),
        }
    );
}
