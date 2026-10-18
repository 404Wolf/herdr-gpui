//! The wrapper run by a local shell, and the relay against a scripted
//! execd stream on loopback.

use super::*;
use crate::opensandbox::relay::{Notice, frame, notice, relay, wrapper};
use std::{
    io::{Cursor, Write},
    net::TcpListener,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};
use tungstenite::Message;

/// What execd would pass the command: the frames the relay sends, as bytes.
fn framed(chunks: &[&[u8]], end: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    for chunk in chunks {
        bytes.extend_from_slice(&frame(chunk)[1..]);
    }
    bytes.extend_from_slice(b"-\n");
    if end {
        bytes.extend_from_slice(b"0\n");
    }
    bytes
}

fn run_wrapper(remote: &str, input: &[u8]) -> (Option<i32>, Vec<u8>) {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(wrapper(remote))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    (output.status.code(), output.stdout)
}

#[test]
fn the_wrapper_passes_framed_bytes_unchanged_and_ends_the_input() {
    let binary: Vec<u8> = (0..=255u8).cycle().take(70_000).collect();
    let input = framed(&[b"line\n", &binary, b"\x00tail"], true);
    let (code, stdout) = run_wrapper("cat; echo; exit 4", &input);
    assert_eq!(code, Some(4));
    let mut expected = b"line\n".to_vec();
    expected.extend_from_slice(&binary);
    expected.extend_from_slice(b"\x00tail\n");
    assert_eq!(stdout, expected);
}

#[test]
fn the_wrapper_ends_the_input_when_its_stream_does() {
    // The relay vanished without an end frame: the input still ends.
    let (code, stdout) = run_wrapper("wc -c | tr -d ' '", &framed(&[b"abc"], false));
    assert_eq!(code, Some(0));
    assert_eq!(stdout, b"3\n");
}

#[test]
fn the_remote_command_is_quoted_once() {
    let (code, stdout) = run_wrapper("printf '%s' \"it's $((1+1))\"", &framed(&[], true));
    assert_eq!(code, Some(0));
    assert_eq!(stdout, b"it's 2");
}

#[test]
fn execd_notices_end_or_fail_the_command() {
    assert_eq!(notice(r#"{"type":"exit","exit_code":3}"#), Notice::Exit(3));
    assert_eq!(
        notice(r#"{"type":"exit","exit_code":300}"#),
        Notice::Exit(255)
    );
    assert_eq!(
        notice(r#"{"type":"error","code":"START_FAILED","error":"no\nshell"}"#),
        Notice::Failed("noshell".into())
    );
    assert_eq!(
        notice(r#"{"type":"connected","mode":"pipe","role":"holder"}"#),
        Notice::Other
    );
}

fn serve(replies: Vec<Message>) -> (String, mpsc::Receiver<Vec<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (sent, received) = mpsc::channel();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        socket
            .send(Message::Text(
                r#"{"type":"connected","mode":"pipe","role":"holder"}"#.into(),
            ))
            .unwrap();
        let mut frames = Vec::new();
        while let Message::Binary(frame) = socket.read().unwrap() {
            let done = frame[..] == b"\x000\n"[..];
            frames.push(frame.to_vec());
            if done {
                break;
            }
        }
        for reply in replies {
            socket.send(reply).unwrap();
        }
        let _ = sent.send(frames);
        thread::sleep(Duration::from_millis(200));
    });
    (format!("ws://{address}/pty/s/ws?pty=0"), received)
}

#[test]
fn the_relay_frames_input_and_splits_output_by_stream() {
    let (url, frames) = serve(vec![
        Message::Binary(vec![0x01, 0, 255].into()),
        Message::Binary(vec![0x02, b'e'].into()),
        Message::Text(r#"{"type":"exit","exit_code":5}"#.into()),
    ]);
    let mut socket = crate::cloud::socket::open(&url::Url::parse(&url).unwrap(), &[]).unwrap();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let code = relay(
        &mut socket,
        crate::cloud::socket::input(Cursor::new(b"hi".to_vec())),
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(code, 5);
    assert_eq!((stdout, stderr), (vec![0, 255], b"e".to_vec()));
    let frames = frames.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(frames, [b"\x002\nhi".to_vec(), b"\x000\n".to_vec()]);
}

#[test]
fn an_execd_error_fails_the_relay() {
    let (url, _) = serve(vec![Message::Text(
        r#"{"type":"error","code":"START_FAILED","error":"bad"}"#.into(),
    )]);
    let mut socket = crate::cloud::socket::open(&url::Url::parse(&url).unwrap(), &[]).unwrap();
    let error = relay(
        &mut socket,
        crate::cloud::socket::input(Cursor::new(Vec::new())),
        &mut Vec::new(),
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(matches!(error, Error::Shell(reason) if reason == "bad"));
}
