//! Shared fixtures; each topic lives in its own file under `tests/`.

use super::*;

mod budgets;
mod declare;
mod next;
mod plan;
mod sandbox;
mod schedule;
mod steps;

const CAPS: &str =
    "herdr.caps { agents_per_run = 2, tokens_per_day = 2_000_000, wall_time = \"3h\" }\n";

/// Loads `body` after a valid `herdr.caps` line.
fn load(body: &str) -> Result<Script> {
    Script::load("test", &format!("{CAPS}{body}"), Limits::default())
}

/// The refusal a load or run stopped with.
fn refusal<T: std::fmt::Debug>(result: Result<T>) -> Refusal {
    result.unwrap_err().refusal().cloned().unwrap()
}

fn fire(reason: Reason) -> Fire {
    Fire {
        reason,
        date: "2026-10-22".to_owned(),
        time: "03:00".to_owned(),
    }
}

/// Runs trigger `index`, every step succeeding, and returns the calls.
fn record(script: &Script, index: usize) -> (Vec<Call>, End) {
    let mut calls = Vec::new();
    let end = script
        .run(index, &fire(Reason::Schedule), |call| {
            calls.push(call.clone());
            Outcome {
                ok: true,
                exit: Some(0),
                ..Outcome::default()
            }
        })
        .unwrap();
    (calls, end)
}
