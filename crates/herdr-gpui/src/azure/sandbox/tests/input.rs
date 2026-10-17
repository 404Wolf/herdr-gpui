#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn input_travels_in_length_frames_and_an_empty_one_ends_it() {
    assert_eq!(relay::input_frame(b"\x00accept\n"), b"8\n\x00accept\n");
    assert_eq!(relay::input_frame(&[]), b"0\n");
}

#[test]
fn the_setup_line_gives_the_command_an_input_that_ends() {
    let line = relay::setup_line(12);
    // The reader stops at the empty frame, and the command reads its FIFO.
    assert!(line.contains("while IFS= read -r n && [ \"$n\" -gt 0 ]; do head -c \"$n\"; done;"));
    assert!(line.contains("eval \"$s\" <\"$f\" 2>\"$e\";"));
    // An interactive shell would stop a background reader of the terminal.
    assert!(line.contains("set +m;"));
    // An interactive bash would take `$!` as a history expansion.
    assert!(!line.contains('!'));
}

#[test]
fn a_stderr_marker_split_anywhere_never_reaches_stdout() {
    let stream: &[u8] = b"\x01herdr-relay-start\x01out\x01\x01herdr-relay-stderr\x01err";
    for at in 0..=stream.len() {
        let mut output = relay::output::Output::default();
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        output
            .push(&stream[..at], &mut stdout, &mut stderr)
            .unwrap();
        output
            .push(&stream[at..], &mut stdout, &mut stderr)
            .unwrap();
        output.flush(&mut stdout).unwrap();
        assert_eq!(stdout, b"out\x01", "split at {at}");
        assert_eq!(stderr, b"err", "split at {at}");
    }
}
