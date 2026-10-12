//! `caffeine`: report, or hold and release as the status bar cup does,
//! optionally for a while. A timed hold ends on its own unless anyone,
//! here or in a window, changed caffeine after it was set.
use super::{Watches, socket};
use crate::{
    caffeine::{self, Cup, Mode},
    control::protocol::{
        ErrorCode, Response,
        app::Switch,
        settings::{MAX_CAFFEINE_SECONDS, Reply},
    },
};
use gpui::{App, Global};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

/// How long a hold or release may take to settle.
const SETTLE: Duration = Duration::from_secs(5);

/// A timed hold: released at `until`, unless caffeine has changed since
/// `changes` holds and releases had settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Timer {
    pub changes: u64,
    pub until: Instant,
}

/// The timer in force; a later request replaces or clears it.
#[derive(Default)]
struct Timed(Option<Timer>);

impl Global for Timed {}

/// How long the hold `timer` was set on has left: `None` once anyone
/// changed caffeine after it was set, or the hold is off, so the timer must
/// not touch it; zero when it is due.
pub(super) fn remaining(timer: Timer, changes: u64, cup: Cup, now: Instant) -> Option<Duration> {
    (timer.changes == changes && cup == Cup::On(Mode::Indefinite))
        .then(|| timer.until.saturating_duration_since(now))
}

pub(super) fn state_name(cup: Cup) -> &'static str {
    match cup {
        Cup::Off => "off",
        Cup::Pending => "pending",
        Cup::On(Mode::Indefinite) => "on",
        Cup::On(Mode::UntilAgentsFinish) => "until_agents_finish",
    }
}

fn state(cx: &App) -> Response {
    let cup = caffeine::cup(cx);
    let left = cx
        .try_global::<Timed>()
        .and_then(|timed| timed.0)
        .and_then(|timer| remaining(timer, caffeine::changes(cx), cup, Instant::now()));
    Response::Settings {
        reply: Reply::Caffeine {
            state: state_name(cup).to_owned(),
            seconds: left.map(|left| left.as_secs()),
        },
    }
}

pub(super) fn serve(
    switch: Option<Switch>,
    seconds: Option<u64>,
    incoming: socket::Incoming,
    watches: &mut Watches,
    cx: &mut App,
) {
    if seconds.is_some_and(|seconds| seconds == 0 || seconds > MAX_CAFFEINE_SECONDS) {
        incoming.respond(Response::error(
            ErrorCode::InvalidRequest,
            "--for must be between one second and a day",
        ));
        return;
    }
    let Some(switch) = switch else {
        incoming.respond(state(cx));
        return;
    };
    let cup = caffeine::cup(cx);
    if cup == Cup::Pending {
        incoming.respond(Response::error(
            ErrorCode::Busy,
            "Caffeine is changing; try again",
        ));
        return;
    }
    let on = matches!(cup, Cup::On(_));
    let wanted = switch.apply(on);
    if seconds.is_some() && !wanted {
        incoming.respond(Response::error(
            ErrorCode::InvalidRequest,
            "--for keeps the machine awake for a while, but this turns caffeine off",
        ));
        return;
    }
    let failure = Rc::new(RefCell::new(None));
    let before = caffeine::changes(cx);
    let toggled = wanted != on;
    if toggled {
        let failure = failure.clone();
        caffeine::toggle(cx, move |error, _| {
            failure.borrow_mut().replace(error);
        });
    }
    watches.add(
        incoming,
        SETTLE,
        move |cx| {
            if let Some(error) = failure.borrow_mut().take() {
                return Some(Response::error(ErrorCode::Failed, error.to_string()));
            }
            // A toggle that never reached the worker reports its failure on
            // a later turn, without ever changing the state.
            if caffeine::cup(cx) == Cup::Pending || (toggled && caffeine::changes(cx) == before) {
                return None;
            }
            let timer = seconds
                .filter(|_| caffeine::cup(cx) == Cup::On(Mode::Indefinite))
                .map(|seconds| Timer {
                    changes: caffeine::changes(cx),
                    until: Instant::now() + Duration::from_secs(seconds),
                });
            arm(timer, cx);
            Some(state(cx))
        },
        Response::error(ErrorCode::Timeout, "Caffeine did not settle in time"),
        cx,
    );
}

/// Puts `timer` in force, replacing any other, and ends its hold when due.
fn arm(timer: Option<Timer>, cx: &mut App) {
    cx.set_global(Timed(timer));
    let Some(timer) = timer else {
        return;
    };
    let clock = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        clock
            .timer(timer.until.saturating_duration_since(Instant::now()))
            .await;
        cx.update(|cx| {
            let current = cx.try_global::<Timed>().and_then(|timed| timed.0);
            if current != Some(timer) {
                return;
            }
            match remaining(
                timer,
                caffeine::changes(cx),
                caffeine::cup(cx),
                Instant::now(),
            ) {
                // Timers may wake a little early.
                Some(left) if !left.is_zero() => arm(Some(timer), cx),
                Some(_) => {
                    cx.set_global(Timed(None));
                    caffeine::toggle(cx, |error, _| {
                        tracing::warn!(%error, "Could not end a timed keep-awake");
                    });
                }
                None => cx.set_global(Timed(None)),
            }
        });
    })
    .detach();
}
