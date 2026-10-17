//! Splits the terminal's single output stream back into what the caller
//! expects. The service only feeds stdin to a process on a terminal, and a
//! terminal merges stdout and stderr, so the shell the relay starts marks the
//! points where the remote command's output begins and where its saved stderr
//! follows.

use super::super::super::{Error, Result};
use std::io::Write;

/// Printed once the terminal is raw. Everything before it is the terminal
/// echoing the setup line, which spells the marker with escapes rather than
/// these bytes.
pub(super) const START: &[u8] = b"\x01herdr-relay-start\x01";
/// Printed after the remote command exits, followed by its stderr.
pub(super) const STDERR: &[u8] = b"\x01herdr-relay-stderr\x01";
/// Setup output kept for a diagnostic when the shell never gets started.
const PREAMBLE_LIMIT: usize = 64 * 1024;
const DIAGNOSTIC_LIMIT: usize = 512;

#[derive(Debug, Default)]
enum Phase {
    /// Waiting for [`START`]; holds what arrived so far.
    #[default]
    Setup,
    /// The remote command's stdout. Holds back an ending that may begin
    /// [`STDERR`], so a marker split across messages never reaches stdout.
    Stdout,
    Stderr,
}

#[derive(Debug, Default)]
pub(in crate::azure::sandbox) struct Output {
    phase: Phase,
    held: Vec<u8>,
}

impl Output {
    /// Route one chunk of terminal output. Returns true when this chunk
    /// carried [`START`]: the shell is now waiting for the remote command.
    pub(in crate::azure::sandbox) fn push(
        &mut self,
        bytes: &[u8],
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<bool> {
        match self.phase {
            Phase::Setup => {
                self.held.extend_from_slice(bytes);
                let Some(at) = find(&self.held, START) else {
                    let excess = self.held.len().saturating_sub(PREAMBLE_LIMIT);
                    self.held.drain(..excess);
                    return Ok(false);
                };
                let rest = self.held.split_off(at + START.len());
                self.held.clear();
                self.phase = Phase::Stdout;
                self.push(&rest, stdout, stderr)?;
                Ok(true)
            }
            Phase::Stdout => {
                self.held.extend_from_slice(bytes);
                if let Some(at) = find(&self.held, STDERR) {
                    write(stdout, &self.held[..at])?;
                    let rest = self.held.split_off(at + STDERR.len());
                    self.held.clear();
                    self.phase = Phase::Stderr;
                    return self.push(&rest, stdout, stderr).map(|_| false);
                }
                // Only an ending that may begin the marker waits, for the
                // next message or until the stream is quiet; see `flush`.
                let ready = self.held.len() - marker_prefix(&self.held);
                write(stdout, &self.held[..ready])?;
                self.held.drain(..ready);
                Ok(false)
            }
            Phase::Stderr => {
                stderr.write_all(bytes).map_err(Error::Process)?;
                Ok(false)
            }
        }
    }

    /// Write stdout held back as a possible start of [`STDERR`]. The shell
    /// prints the marker in one write, so a stream quiet for a poll, or a
    /// command that exited, rules it out; the bridge may be waiting on the
    /// bytes, which can end a binary frame.
    pub(in crate::azure::sandbox) fn flush(&mut self, stdout: &mut impl Write) -> Result<()> {
        if !matches!(self.phase, Phase::Stdout) {
            return Ok(());
        }
        write(stdout, &self.held)?;
        self.held.clear();
        Ok(())
    }

    pub(in crate::azure::sandbox) fn started(&self) -> bool {
        !matches!(self.phase, Phase::Setup)
    }

    /// What the terminal showed before the shell failed to start.
    pub(in crate::azure::sandbox) fn diagnostic(&self) -> String {
        let text = String::from_utf8_lossy(&self.held);
        let text = text.trim();
        let skip = text.chars().count().saturating_sub(DIAGNOSTIC_LIMIT);
        text.chars().skip(skip).collect()
    }
}

fn write(stdout: &mut impl Write, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    stdout.write_all(bytes).map_err(Error::Process)?;
    stdout.flush().map_err(Error::Process)
}

/// The length of the longest ending of `bytes` that begins [`STDERR`]
/// without completing it.
fn marker_prefix(bytes: &[u8]) -> usize {
    (1..STDERR.len())
        .rev()
        .find(|&len| bytes.ends_with(&STDERR[..len]))
        .unwrap_or(0)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
