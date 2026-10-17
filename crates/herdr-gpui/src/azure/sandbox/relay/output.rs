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
    /// The remote command's stdout. Holds the bytes already written that may
    /// begin [`STDERR`], so a marker split across messages is still found.
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
                let seen = self.held.len();
                self.held.extend_from_slice(bytes);
                if let Some(at) = find(&self.held, STDERR) {
                    // Bytes of a marker split across messages already went
                    // out as stdout; the command had exited by then.
                    let output = &self.held[seen.min(at)..at];
                    write(stdout, output)?;
                    let rest = self.held.split_off(at + STDERR.len());
                    self.held.clear();
                    self.phase = Phase::Stderr;
                    return self.push(&rest, stdout, stderr).map(|_| false);
                }
                // Never hold stdout back: the bridge may be waiting for it.
                write(stdout, bytes)?;
                let keep = STDERR.len() - 1;
                let excess = self.held.len().saturating_sub(keep);
                self.held.drain(..excess);
                Ok(false)
            }
            Phase::Stderr => {
                stderr.write_all(bytes).map_err(Error::Process)?;
                Ok(false)
            }
        }
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

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
