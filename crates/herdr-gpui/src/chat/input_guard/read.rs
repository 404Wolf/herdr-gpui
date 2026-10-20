//! Detection reads contain no color on Herdr 0.9.3, even with --format ansi.
//! Use viewport styles only when bracketed by unchanged, bottom-of-history
//! metadata and live-screen reads. Equal words alone cannot prove a placeholder.
use super::{empty, plain};
use crate::{
    chat::{Error, Result},
    teleport::{Envelope, Host},
};
use herdr_client::protocol::PaneSurfaceScrollMetrics;
use serde::Deserialize;
use std::{sync::atomic::AtomicBool, time::Duration};

const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
struct Viewport {
    pane_id: String,
    terminal_id: String,
    revision: u64,
    scroll: Option<PaneSurfaceScrollMetrics>,
}
#[derive(Deserialize)]
struct PaneResult {
    pane: Viewport,
}

fn viewport(host: &Host, pane: &str, stop: &AtomicBool) -> Result<Viewport> {
    let output = host
        .herdr_exec(&["pane", "get", pane], DEADLINE, stop)
        .map_err(Error::Query)?;
    serde_json::from_slice::<Envelope<PaneResult>>(&output)
        .map(|r| r.result.pane)
        .map_err(Error::Decode)
}

fn read(host: &Host, pane: &str, source: &str, format: &str, stop: &AtomicBool) -> Result<Vec<u8>> {
    host.herdr_exec(
        &[
            "agent", "read", pane, "--source", source, "--format", format, "--lines", "100",
        ],
        DEADLINE,
        stop,
    )
    .map_err(Error::Query)
}

pub(in crate::chat) fn screen(
    host: &Host,
    pane: &str,
    terminal: &str,
    stop: &AtomicBool,
) -> Result<Vec<u8>> {
    let before = read(host, pane, "detection", "text", stop)?;
    if empty(&String::from_utf8_lossy(&before)) {
        return Ok(before);
    }
    let first = viewport(host, pane, stop)?;
    if !at_bottom(&first, pane, terminal) {
        return Err(Error::InputOccupied);
    }
    let colors = read(host, pane, "visible", "ansi", stop)?;
    let last = viewport(host, pane, stop)?;
    let after = read(host, pane, "detection", "text", stop)?;
    if verified(&first, &last, pane, terminal, &before, &colors, &after) {
        Ok(colors)
    } else {
        Err(Error::InputOccupied)
    }
}

fn at_bottom(view: &Viewport, pane: &str, terminal: &str) -> bool {
    view.pane_id == pane
        && view.terminal_id == terminal
        && view.scroll.is_some_and(|s| s.offset_from_bottom == 0)
}

fn verified(
    first: &Viewport,
    last: &Viewport,
    pane: &str,
    terminal: &str,
    before: &[u8],
    colors: &[u8],
    after: &[u8],
) -> bool {
    if first != last || !at_bottom(first, pane, terminal) || before != after {
        return false;
    }
    let colors = String::from_utf8_lossy(colors);
    let Some(live) = plain(&String::from_utf8_lossy(after)) else {
        return false;
    };
    plain(&colors).as_deref() == Some(live.as_str()) && empty(&colors)
}

#[cfg(test)]
mod tests;
