//! This executable as a sprite's standard streams. Cloud devices reach their
//! machines by running a command whose stdin and stdout are the remote shell's,
//! the way `ssh host COMMAND` does, and Herdr's bridge, probe, installer, and
//! Teleport's scripts all append their remote command to it. A sprite offers
//! no SSH server, only the API's exec stream: a WebSocket whose binary frames
//! carry one stream byte and then data. The app runs itself with [`HELPER`]
//! to translate between the two.
//!
//! Frames from the relay: `0x00` and stdin bytes, and `0x04` once stdin ends.
//! Frames to it: `0x01` stdout, `0x02` stderr, and `0x03` with the exit code;
//! JSON text frames carry session and port notices, and an `exit` message.
//! Without a terminal the bytes pass unchanged, so the bridge's binary
//! protocol survives the trip.

use super::{Error, Result, http::authorization, store};
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    process::ExitCode,
    sync::{
        Arc,
        mpsc::{self, Receiver, TryRecvError},
    },
    time::Duration,
};
use tungstenite::{
    Message, WebSocket, client::IntoClientRequest, http::HeaderValue, protocol::WebSocketConfig,
};
use url::Url;

/// The argument that makes this executable a sprite relay.
pub(crate) const HELPER: &str = "__sprites-exec";
/// What `ssh` exits with when the connection, not the command, failed. The
/// bridge and installer read it the same way here.
const CONNECTION_FAILED: u8 = 255;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a read waits before the relay forwards pending input.
const POLL: Duration = Duration::from_millis(20);
const CHUNK: usize = 32 * 1024;
const MESSAGE_LIMIT: usize = 4 * 1024 * 1024;
const QUEUED_CHUNKS: usize = 64;
/// Input chunks sent before the relay reads again, so steady input, such as
/// a Git bundle, never keeps the command's output waiting.
const CHUNKS_PER_PASS: usize = 4;

const STDIN: u8 = 0x00;
const STDOUT: u8 = 0x01;
const STDERR: u8 = 0x02;
const EXIT: u8 = 0x03;
const STDIN_EOF: u8 = 0x04;

/// The socket under the exec stream: TLS to the API, or plain to a loopback
/// API root, which only tests use.
pub(super) enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Stream {
    fn tcp(&self) -> &TcpStream {
        match self {
            Self::Plain(tcp) => tcp,
            Self::Tls(tls) => &tls.sock,
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(tcp) => tcp.read(buf),
            Self::Tls(tls) => tls.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(tcp) => tcp.write(buf),
            Self::Tls(tls) => tls.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(tcp) => tcp.flush(),
            Self::Tls(tls) => tls.flush(),
        }
    }
}

/// Runs this executable as a relay when `args` asks for it, before anything
/// of the app starts.
pub(crate) fn run_helper(args: &[OsString]) -> Option<ExitCode> {
    let (first, rest) = args.split_first()?;
    if first != HELPER {
        return None;
    }
    let [account, sprite, remote @ ..] = rest else {
        eprintln!("Usage: herdr-gpui {HELPER} ACCOUNT SPRITE COMMAND...");
        return Some(ExitCode::from(2));
    };
    if remote.is_empty() {
        eprintln!("Usage: herdr-gpui {HELPER} ACCOUNT SPRITE COMMAND...");
        return Some(ExitCode::from(2));
    }
    Some(match run(account, sprite, remote) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("herdr-gpui: {error}");
            ExitCode::from(CONNECTION_FAILED)
        }
    })
}

fn run(account: &OsString, sprite: &OsString, remote: &[OsString]) -> Result<u8> {
    let account = account.to_str().ok_or(Error::Encoding("account"))?;
    let sprite = sprite.to_str().ok_or(Error::Encoding("sprite name"))?;
    // Like `ssh`, the remote command is its arguments joined by spaces.
    let command = remote
        .iter()
        .map(|part| part.to_str().ok_or(Error::Encoding("remote command")))
        .collect::<Result<Vec<_>>>()?
        .join(" ");
    let settings = super::connect::settings(account)?;
    let url = settings.exec_url(sprite, &command)?;
    let mut socket = open(&url, &store::token(&settings)?)?;
    socket
        .get_ref()
        .tcp()
        .set_read_timeout(Some(POLL))
        .map_err(Error::Connect)?;
    let code = relay(
        &mut socket,
        input(io::stdin()),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )?;
    let _ = socket.close(None);
    Ok(code)
}

/// Open the exec stream at `url` with `token`.
pub(super) fn open(url: &Url, token: &secrecy::SecretString) -> Result<WebSocket<Stream>> {
    let host = url
        .host_str()
        .ok_or(Error::Url("sprites.api_url"))?
        .to_owned();
    let port = url
        .port_or_known_default()
        .ok_or(Error::Url("sprites.api_url"))?;
    let mut request = url.as_str().into_client_request().map_err(Error::Stream)?;
    let header = authorization(token)?;
    let mut value = HeaderValue::from_bytes(header.as_bytes()).map_err(Error::StreamHeader)?;
    value.set_sensitive(true);
    request.headers_mut().insert("Authorization", value);
    let address = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(Error::Connect)?
        .next()
        .ok_or(Error::Url("sprites.api_url"))?;
    let tcp = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(Error::Connect)?;
    tcp.set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(Error::Connect)?;
    tcp.set_write_timeout(Some(HANDSHAKE_TIMEOUT))
        .map_err(Error::Connect)?;
    tcp.set_nodelay(true).map_err(Error::Connect)?;
    let stream = if url.scheme() == "wss" {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(Error::Tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(host)
            .map_err(|_| Error::Url("sprites.api_url"))?;
        let connection = rustls::ClientConnection::new(Arc::new(tls), name).map_err(Error::Tls)?;
        Stream::Tls(Box::new(rustls::StreamOwned::new(connection, tcp)))
    } else {
        Stream::Plain(tcp)
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MESSAGE_LIMIT))
        .max_frame_size(Some(MESSAGE_LIMIT));
    let (socket, _) = tungstenite::client::client_with_config(request, stream, Some(config))
        .map_err(|error| match error {
            tungstenite::HandshakeError::Failure(error) => Error::Stream(error),
            tungstenite::HandshakeError::Interrupted(_) => Error::Closed,
        })?;
    Ok(socket)
}

/// Read `source` on its own thread, in chunks, then `None` at its end. The
/// channel is bounded, so a slow sprite holds the reader back.
pub(super) fn input(mut source: impl Read + Send + 'static) -> Receiver<Option<Vec<u8>>> {
    let (sender, receiver) = mpsc::sync_channel(QUEUED_CHUNKS);
    let spawned = std::thread::Builder::new()
        .name("herdr-sprites-stdin".into())
        .spawn(move || {
            let mut chunk = vec![0; CHUNK];
            loop {
                match source.read(&mut chunk) {
                    Ok(0) | Err(_) => {
                        let _ = sender.send(None);
                        return;
                    }
                    Ok(read) => {
                        if sender.send(Some(chunk[..read].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the sprite relay's input reader");
    }
    receiver
}

/// The exit code in an `exit` text message, if `text` is one.
fn exit_message(text: &str) -> Option<u8> {
    #[derive(serde::Deserialize)]
    struct Exit {
        #[serde(rename = "type")]
        kind: String,
        exit_code: i64,
    }
    let exit = serde_json::from_str::<Exit>(text).ok()?;
    (exit.kind == "exit").then(|| u8::try_from(exit.exit_code).unwrap_or(CONNECTION_FAILED))
}

/// Pass `input` to the command and its output to `stdout` and `stderr` until
/// it exits, and return its exit code. The socket's reads must time out, so
/// input keeps flowing while the command is quiet.
pub(super) fn relay<S: Read + Write>(
    socket: &mut WebSocket<S>,
    input: Receiver<Option<Vec<u8>>>,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
) -> Result<u8> {
    let mut input = Some(input);
    loop {
        for _ in 0..CHUNKS_PER_PASS {
            let Some(receiver) = &input else {
                break;
            };
            let frame = match receiver.try_recv() {
                Ok(Some(chunk)) => {
                    let mut frame = Vec::with_capacity(chunk.len() + 1);
                    frame.push(STDIN);
                    frame.extend_from_slice(&chunk);
                    frame
                }
                Ok(None) => {
                    input = None;
                    vec![STDIN_EOF]
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    input = None;
                    break;
                }
            };
            // A write that times out has queued its frame; flushing goes on
            // below, between reads.
            match socket.write(Message::Binary(frame.into())) {
                Ok(()) => {}
                Err(tungstenite::Error::Io(error)) if pending(&error) => {}
                Err(error) => return Err(Error::Stream(error)),
            }
        }
        match socket.flush() {
            Ok(()) => {}
            Err(tungstenite::Error::Io(error)) if pending(&error) => {}
            Err(error) => return Err(Error::Stream(error)),
        }
        match socket.read() {
            Ok(Message::Binary(frame)) => match frame.split_first() {
                Some((&STDOUT, data)) => {
                    stdout.write_all(data).map_err(Error::Connect)?;
                    stdout.flush().map_err(Error::Connect)?;
                }
                Some((&STDERR, data)) => {
                    stderr.write_all(data).map_err(Error::Connect)?;
                    stderr.flush().map_err(Error::Connect)?;
                }
                Some((&EXIT, code)) => return Ok(code.first().copied().unwrap_or(0)),
                _ => {}
            },
            Ok(Message::Text(text)) => {
                if let Some(code) = exit_message(text.as_str()) {
                    return Ok(code);
                }
            }
            Ok(Message::Close(_)) => return Err(Error::Closed),
            Ok(_) => {}
            Err(tungstenite::Error::Io(error)) if pending(&error) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Err(Error::Closed);
            }
            Err(error) => return Err(Error::Stream(error)),
        }
    }
}

/// A read or write that timed out with nothing to do, not a failure.
fn pending(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}
