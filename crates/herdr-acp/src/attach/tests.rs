#![allow(clippy::unwrap_used, clippy::expect_used)]

use crossbeam_channel::Receiver;
use herdr_chat_protocol::{ClientKind, Command, CommandId};
use herdr_client::private_endpoint::{Reader, Writer};

use super::*;

const WAIT: Duration = Duration::from_secs(10);

struct Fixture {
    address: PathBuf,
    inputs: Receiver<Input>,
    /// Held to keep listening; only the Unix test reads it.
    #[cfg_attr(windows, allow(dead_code))]
    server: Option<Server>,
    #[cfg(unix)]
    dir: tempfile::TempDir,
}

fn server() -> Fixture {
    let id = EndpointId::random();
    #[cfg(unix)]
    let dir = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let address = herdr_chat_protocol::endpoint_path(&dir.path().join("acp"), &id);
    #[cfg(windows)]
    let address = herdr_chat_protocol::endpoint_address(&id).unwrap();
    let (inputs, receiver) = bounded(16);
    let server = Server::listen(&address, inputs).unwrap();
    Fixture {
        address,
        inputs: receiver,
        server: Some(server),
        #[cfg(unix)]
        dir,
    }
}

fn connect(fixture: &Fixture) -> (Reader, Writer) {
    let (mut reader, writer, _closer) = Connection::connect(&fixture.address, WAIT)
        .unwrap()
        .split()
        .unwrap();
    reader.set_timeout(Some(WAIT)).unwrap();
    (reader, writer)
}

fn hello(writer: &mut Writer, version: u32) {
    write_frame(
        writer,
        &ClientMessage::Hello {
            client: ClientKind::Gui,
            since: None,
            version,
        },
    )
    .unwrap();
}

#[test]
fn a_client_attaches_sends_commands_and_detaches() {
    let fixture = server();
    let (mut reader, mut writer) = connect(&fixture);
    hello(&mut writer, VERSION);
    let Input::Attach(attach) = fixture.inputs.recv_timeout(WAIT).unwrap() else {
        panic!("expected attach");
    };
    assert!(!attach.local);
    attach
        .outbox
        .send(ServerMessage::Ack {
            id: CommandId::random(),
            rejected: None,
        })
        .unwrap();
    let message = read_frame::<_, ServerMessage>(&mut reader).unwrap();
    assert!(matches!(message, Some(ServerMessage::Ack { .. })));

    let id = CommandId::random();
    write_frame(
        &mut writer,
        &ClientMessage::Command {
            command: Command::Cancel,
            id,
        },
    )
    .unwrap();
    let Input::Command {
        command,
        conn,
        id: received,
    } = fixture.inputs.recv_timeout(WAIT).unwrap()
    else {
        panic!("expected a command");
    };
    assert_eq!(
        (command, conn, received),
        (Command::Cancel, attach.conn, id)
    );
    drop((reader, writer));
    assert!(matches!(
        fixture.inputs.recv_timeout(WAIT).unwrap(),
        Input::Detach(conn) if conn == attach.conn
    ));
}

#[test]
fn another_protocol_version_is_turned_away() {
    let fixture = server();
    let (mut reader, mut writer) = connect(&fixture);
    hello(&mut writer, VERSION + 1);
    let message = read_frame::<_, ServerMessage>(&mut reader).unwrap();
    assert_eq!(
        message,
        Some(ServerMessage::Bye {
            reason: ByeReason::Version { supported: VERSION }
        })
    );
    assert!(fixture.inputs.try_recv().is_err());
}

#[test]
fn a_command_before_hello_is_a_handshake_error() {
    let fixture = server();
    let (mut reader, mut writer) = connect(&fixture);
    write_frame(
        &mut writer,
        &ClientMessage::Command {
            command: Command::Cancel,
            id: CommandId::random(),
        },
    )
    .unwrap();
    let message = read_frame::<_, ServerMessage>(&mut reader).unwrap();
    assert_eq!(
        message,
        Some(ServerMessage::Bye {
            reason: ByeReason::Handshake
        })
    );
}

#[test]
fn dropping_the_session_side_closes_the_connection() {
    let fixture = server();
    let (mut reader, mut writer) = connect(&fixture);
    hello(&mut writer, VERSION);
    let Input::Attach(attach) = fixture.inputs.recv_timeout(WAIT).unwrap() else {
        panic!("expected attach");
    };
    drop(attach);
    assert_eq!(read_frame::<_, ServerMessage>(&mut reader).unwrap(), None);
}

#[cfg(unix)]
#[test]
fn stale_sockets_are_swept_and_the_live_one_removed_on_drop() {
    use std::os::unix::net::UnixListener;
    let mut fixture = server();
    let chat = fixture.dir.path().join("acp");
    let stale = herdr_chat_protocol::endpoint_path(&chat, &EndpointId::random());
    drop(UnixListener::bind(&stale).unwrap());
    let other = chat.join("notes.sock");
    drop(UnixListener::bind(&other).unwrap());
    sweep(&chat);
    assert!(!stale.exists());
    assert!(other.exists(), "only endpoint sockets are swept");
    assert!(fixture.address.exists(), "a live socket is kept");
    fixture.server.take();
    assert!(!fixture.address.exists());
}
