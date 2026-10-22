//! Automation scripts for Herdr: sandboxed Luau that declares triggers and
//! caps, and calls steps from a closed catalog. Steps are a script's only
//! effects, so the same script always makes the same calls, and
//! [`Plan::of`] can show everything it can do before anyone approves it.
//!
//! ```lua
//! herdr.caps { agents_per_run = 1, tokens_per_day = 2_000_000, wall_time = "3h" }
//!
//! herdr.at("03:00", { tz = "Europe/Paris" }, function(fire)
//!   pull("main")
//!   run("validation")
//!   agent { harness = "claude", prompt = "..." }
//! end)
//! ```

mod args;
mod caps;
mod error;
mod plan;
mod schedule;
mod script;
mod step;
mod vm;

pub use caps::Caps;
pub use error::{Budget, Error, Problem, Refusal, Result};
pub use plan::{FAKE_LOG_TAIL, Failure, Plan, PlannedStep, TriggerPlan};
pub use schedule::{
    ClockTime, Cron, Days, Due, LATE_AFTER, Schedule, Zone, due, format_duration, parse_duration,
};
pub use script::{End, Fire, MAX_SCRIPT_BYTES, Reason, Script, Trigger};
pub use step::{Call, Kind, Outcome, Step};
pub use vm::Limits;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
