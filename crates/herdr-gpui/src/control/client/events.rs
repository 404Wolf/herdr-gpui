//! `events` and `wait`.
use super::{Context, Output, unexpected};
use crate::{
    cli::control::{EventKind, WaitFor},
    control::protocol::{
        Request, Response,
        events::{Condition, Event, EventsOp, Reply},
    },
};
use std::io::Write as _;

/// How long one request of `events --follow` waits before asking again.
const FOLLOW_WAIT: u64 = 300;

fn line(event: &Event) -> String {
    let detail = event
        .detail
        .as_deref()
        .map(|detail| format!(" {detail}"))
        .unwrap_or_default();
    super::printable(
        &format!(
            "{} {} {}{detail}",
            event.seq,
            event.kind.name(),
            event.subject
        ),
        4096,
    )
}

fn poll(
    after: Option<u64>,
    wait: u64,
    kinds: &[EventKind],
    context: &Context,
) -> crate::Result<(Vec<Event>, u64)> {
    match context.call(
        Request::Events,
        EventsOp::Poll {
            after,
            wait_seconds: wait,
            kinds: kinds.to_vec(),
        },
    )? {
        Response::Events {
            reply: Reply::Events { events, next },
        } => Ok((events, next)),
        response => Err(unexpected(response)),
    }
}

pub(super) fn events(
    after: Option<u64>,
    follow: bool,
    wait: u64,
    kinds: Vec<EventKind>,
    context: &Context,
) -> crate::Result<Output> {
    if !follow {
        let (events, next) = poll(after, wait, &kinds, context)?;
        let text = events.iter().map(line).collect::<Vec<_>>().join("\n");
        let output = Output::data(
            if events.is_empty() {
                format!("No events; next sequence number {next}")
            } else {
                text
            },
            &serde_json::json!({ "events": events, "next": next }),
        )?;
        return Ok(if events.is_empty() {
            output.nothing()
        } else {
            output
        });
    }
    // One line per event, JSON lines with --json, until interrupted or the
    // app goes away.
    let mut after = after;
    let mut stdout = std::io::stdout().lock();
    loop {
        let (events, next) = poll(after, FOLLOW_WAIT, &kinds, context)?;
        for event in &events {
            let text = if context.json {
                serde_json::to_string(event)?
            } else {
                line(event)
            };
            writeln!(stdout, "{text}")?;
        }
        stdout.flush()?;
        after = Some(next);
    }
}

pub(super) fn wait(what: WaitFor, timeout: u64, context: &Context) -> crate::Result<Output> {
    let condition = match what {
        WaitFor::Feedback => return super::browser::feedback(timeout, context),
        WaitFor::Window => Condition::Window,
        WaitFor::Online => Condition::Online,
        WaitFor::TabClosed(id) => Condition::TabClosed { id },
    };
    match context.call(
        Request::Events,
        EventsOp::Wait {
            condition,
            wait_seconds: timeout,
        },
    )? {
        Response::Events {
            reply: Reply::Waited { met: true },
        } => Ok(Output::data("Done", &serde_json::json!({ "met": true }))?),
        Response::Events {
            reply: Reply::Waited { met: false },
        } => Ok(Output::data(
            format!("Still waiting after {timeout} seconds"),
            &serde_json::json!({ "met": false }),
        )?
        .nothing()),
        response => Err(unexpected(response)),
    }
}
