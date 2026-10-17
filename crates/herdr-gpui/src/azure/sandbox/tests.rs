#![allow(clippy::unwrap_used)]
use super::{
    super::{Error, Settings, machine::Machine},
    access,
    api::{self, State},
    relay::{self, Event},
};
use crate::config::AzureConfig;
use base64::Engine as _;
use std::ffi::OsString;

fn settings() -> Settings {
    let config = AzureConfig {
        subscription_id: Some("11111111-1111-1111-1111-111111111111".into()),
        resource_group: Some("my-rg".into()),
        sandbox_group: Some("my-group".into()),
        sandbox_location: Some("EastUS2".into()),
        ..Default::default()
    };
    Settings::resolve(&config, |_| None).unwrap().unwrap()
}

#[test]
fn saved_ids_tell_vms_from_sandboxes() {
    assert_eq!(
        Machine::parse("herdr-quiet-otter"),
        Machine::Vm("herdr-quiet-otter")
    );
    let id = Machine::sandbox_id("4f1c");
    assert_eq!(id, "sandbox.4f1c");
    assert_eq!(Machine::parse(&id), Machine::Sandbox("4f1c"));
    assert_eq!(Machine::parse(&id).noun(), "sandbox");
}

#[test]
fn sandbox_settings_default_and_normalize() {
    let settings = settings();
    assert_eq!(settings.sandbox_group, "my-group");
    assert_eq!(settings.sandbox_location, "eastus2");
    assert_eq!(settings.sandbox_image, "ubuntu");
}

#[test]
fn the_granted_identity_comes_from_the_token_claims() {
    let encode = |json: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json);
    let token = format!(
        "{}.{}.signature",
        encode(r#"{"alg":"RS256"}"#),
        encode(r#"{"oid":"22222222-2222-2222-2222-222222222222","idtyp":"user"}"#)
    );
    let principal = access::principal(&token).unwrap();
    assert_eq!(principal.id, "22222222-2222-2222-2222-222222222222");
    assert!(matches!(
        access::principal("not-a-token"),
        Err(Error::Field(_))
    ));
}

#[test]
fn sandbox_region_follows_the_vm_region_unless_set() {
    let config = AzureConfig {
        subscription_id: Some("11111111-1111-1111-1111-111111111111".into()),
        location: Some("Central US".into()),
        ..Default::default()
    };
    let settings = Settings::resolve(&config, |_| None).unwrap().unwrap();
    assert_eq!(settings.sandbox_location, "centralus");
}

#[test]
fn sandbox_settings_reject_values_that_would_leave_their_url_segment() {
    for (config, field) in [
        (
            AzureConfig {
                sandbox_group: Some("a/b".into()),
                ..Default::default()
            },
            "azure.sandbox_group",
        ),
        (
            AzureConfig {
                sandbox_location: Some("evil.example.com/".into()),
                ..Default::default()
            },
            "azure.sandbox_location",
        ),
        (
            AzureConfig {
                sandbox_image: Some("..".into()),
                ..Default::default()
            },
            "azure.sandbox_image",
        ),
    ] {
        let config = AzureConfig {
            subscription_id: Some("11111111-1111-1111-1111-111111111111".into()),
            ..config
        };
        let error = Settings::resolve(&config, |_| None).unwrap_err();
        assert!(
            matches!(error, Error::Field(name) if name == field),
            "{error:?}"
        );
    }
}

#[test]
fn exec_stream_url_targets_the_regional_data_plane() {
    let url = api::exec_stream_url(&settings(), "abc-123").unwrap();
    assert_eq!(
        url,
        "wss://management.eastus2.azuredevcompute.io/subscriptions/11111111-1111-1111-1111-111111111111/resourceGroups/my-rg/sandboxGroups/my-group/sandboxes/abc-123/exec/stream"
    );
    assert!(matches!(
        api::exec_stream_url(&settings(), "../escape"),
        Err(Error::Field(_))
    ));
}

#[test]
fn sandbox_states_group_into_what_connecting_needs() {
    assert_eq!(api::parse_state("Running"), State::Running);
    assert_eq!(api::parse_state("Suspended"), State::Stopped);
    assert_eq!(api::parse_state("Stopped"), State::Stopped);
    assert_eq!(api::parse_state("Resuming"), State::Changing);
    assert_eq!(api::parse_state("Failed"), State::Failed);
    assert_eq!(api::parse_state("Deleting"), State::Deleting);
}

#[test]
fn remote_command_is_joined_like_ssh_and_read_by_length() {
    assert_eq!(relay::remote_command(&[]).unwrap(), b"");
    let remote = [OsString::from("/bin/sh -c 'echo hi\nexit 3'")];
    let command = relay::remote_command(&remote).unwrap();
    assert_eq!(command, b"/bin/sh -c 'echo hi\nexit 3'");
    let line = relay::setup_line(command.len());
    assert!(line.starts_with("stty raw -echo -iexten || exit 255;"));
    assert!(line.contains(&format!("s=$(head -c {});", command.len())));
    assert!(line.ends_with("exit $c\n"));
    // The echoed line must not contain the markers' bytes.
    assert!(!line.contains('\x01'));
    assert_eq!(line.matches('\n').count(), 1);
}

#[test]
fn stdin_is_sent_as_base64() {
    let message: serde_json::Value =
        serde_json::from_str(&relay::stdin_message(b"\x00accept\n")).unwrap();
    assert_eq!(message["type"], "stdin");
    let data = message["data"].as_str().unwrap();
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap(),
        b"\x00accept\n"
    );
}

#[test]
fn stream_messages_become_relay_events() {
    assert_eq!(
        relay::event(r#"{"type":"stdout","data":"aGk="}"#).unwrap(),
        Event::Stdout(b"hi".to_vec())
    );
    assert_eq!(
        relay::event(r#"{"type":"stderr","data":""}"#).unwrap(),
        Event::Stderr(Vec::new())
    );
    assert_eq!(
        relay::event(r#"{"type":"exit_code","exitCode":3}"#).unwrap(),
        Event::Exit(3)
    );
    // An exit code a process cannot have reads as a failed connection.
    assert_eq!(
        relay::event(r#"{"type":"exit_code","exitCode":-1}"#).unwrap(),
        Event::Exit(255)
    );
    assert!(matches!(
        relay::event(r#"{"type":"error","message":"denied"}"#).unwrap(),
        Event::Failed(_)
    ));
    assert_eq!(relay::event(r#"{"type":"ping"}"#).unwrap(), Event::Ignore);
    assert!(matches!(
        relay::event(r#"{"type":"stdout","data":"%%"}"#),
        Err(Error::StreamMessage)
    ));
    assert!(matches!(relay::event("not json"), Err(Error::Json(_))));
}

#[test]
fn helper_ignores_other_arguments_and_rejects_missing_ones() {
    assert_eq!(relay::run_helper(&[OsString::from("--session")]), None);
    assert_eq!(
        relay::run_helper(&[OsString::from(relay::HELPER), OsString::from("only")]),
        Some(std::process::ExitCode::from(2))
    );
}

#[test]
fn output_drops_the_setup_echo_and_splits_saved_stderr() {
    let mut output = relay::output::Output::default();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    assert!(
        !output
            .push(b"stty raw -echo\r\n\x01herdr-re", &mut stdout, &mut stderr)
            .unwrap()
    );
    assert!(!output.started());
    assert!(
        output
            .push(b"lay-start\x01\x00bin", &mut stdout, &mut stderr)
            .unwrap()
    );
    assert!(output.started());
    // A stderr marker split across messages is still found.
    assert!(
        !output
            .push(b"ary\r\n\x01herdr-relay-std", &mut stdout, &mut stderr)
            .unwrap()
    );
    assert!(
        !output
            .push(b"err\x01warn\n", &mut stdout, &mut stderr)
            .unwrap()
    );
    assert!(!output.push(b"more\n", &mut stdout, &mut stderr).unwrap());
    assert_eq!(stdout, b"\x00binary\r\n\x01herdr-relay-std");
    assert_eq!(stderr, b"warn\nmore\n");
}

#[test]
fn output_never_holds_stdout_back() {
    let mut output = relay::output::Output::default();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    output
        .push(
            b"\x01herdr-relay-start\x01frame\x01",
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
    assert_eq!(stdout, b"frame\x01");
}

#[test]
fn output_reports_what_a_shell_that_never_started_printed() {
    let mut output = relay::output::Output::default();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    output
        .push(
            b"exec failed: /bin/sh: not found\n",
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
    assert_eq!(output.diagnostic(), "exec failed: /bin/sh: not found");
    assert!(stdout.is_empty() && stderr.is_empty());
}

/// A local exec stream peer: it checks what the relay sends, answers like the
/// service's terminal, and reports the remote command and input it was given.
#[test]
fn relay_runs_a_command_and_carries_its_streams_and_exit_code() {
    use std::{net::TcpListener, sync::mpsc, time::Duration};
    use tungstenite::Message;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let service = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        let next = |socket: &mut tungstenite::WebSocket<_>| -> serde_json::Value {
            loop {
                if let Message::Text(text) = socket.read().unwrap() {
                    return serde_json::from_str(text.as_str()).unwrap();
                }
            }
        };
        let decode = |message: &serde_json::Value| {
            base64::engine::general_purpose::STANDARD
                .decode(message["data"].as_str().unwrap())
                .unwrap()
        };
        let send = |socket: &mut tungstenite::WebSocket<_>, kind: &str, data: &[u8]| {
            let data = base64::engine::general_purpose::STANDARD.encode(data);
            let value = serde_json::json!({ "type": kind, "data": data });
            socket.send(Message::text(value.to_string())).unwrap();
        };
        let start = next(&mut socket);
        assert_eq!(start["type"], "start");
        assert_eq!(start["start"]["command"], "/bin/sh");
        assert_eq!(start["start"]["tty"], true);
        let setup = decode(&next(&mut socket));
        // The terminal echoes the setup line, then the shell starts.
        send(&mut socket, "stdout", &setup);
        send(&mut socket, "stdout", b"\x01herdr-relay-start\x01ready\n");
        let command = decode(&next(&mut socket));
        let reply = decode(&next(&mut socket));
        send(&mut socket, "stderr", b"\x01herdr-relay-stderr\x01warn\n");
        let exit = serde_json::json!({ "type": "exit_code", "exitCode": 3 });
        socket.send(Message::text(exit.to_string())).unwrap();
        let _ = socket.read();
        (setup, command, reply)
    });

    let stream = std::net::TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let (mut socket, _) = tungstenite::client(format!("ws://{address}/"), stream).unwrap();
    let command = relay::start(&mut socket, &[OsString::from("/bin/sh -c 'probe'")]).unwrap();
    let (send, input) = mpsc::sync_channel(4);
    // Queued before the shell starts, so it must follow the command.
    send.send(Some(b"accept\n".to_vec())).unwrap();
    // Input ending must not end the command: a probe has none at all.
    send.send(None).unwrap();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = relay::relay(&mut socket, &command, input, &mut stdout, &mut stderr).unwrap();

    assert_eq!(code, std::process::ExitCode::from(3));
    assert_eq!(stdout, b"ready\n");
    assert_eq!(stderr, b"warn\n");
    let (setup, command, reply) = service.join().unwrap();
    assert_eq!(setup, relay::setup_line(command.len()).as_bytes());
    assert_eq!(command, b"/bin/sh -c 'probe'");
    assert_eq!(reply, b"accept\n");
}

#[test]
fn a_shell_that_never_starts_is_a_failed_connection() {
    use std::{net::TcpListener, sync::mpsc, time::Duration};
    use tungstenite::Message;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let service = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(b"exec failed\n");
        for value in [
            serde_json::json!({ "type": "stderr", "data": data }),
            // What the service reports when it could not start the shell.
            serde_json::json!({ "type": "exit_code", "exitCode": 0 }),
        ] {
            socket.send(Message::text(value.to_string())).unwrap();
        }
        while socket.read().is_ok() {}
    });

    let stream = std::net::TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_millis(20)))
        .unwrap();
    let (mut socket, _) = tungstenite::client(format!("ws://{address}/"), stream).unwrap();
    let (_send, input) = mpsc::sync_channel(1);
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let error = relay::relay(&mut socket, b"true", input, &mut stdout, &mut stderr).unwrap_err();
    assert!(
        matches!(&error, Error::Sandbox(text) if text == "exec failed"),
        "{error:?}"
    );
    drop(socket);
    service.join().unwrap();
}
