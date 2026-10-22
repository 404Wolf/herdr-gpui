//! Automation scripts for Herdr: sandboxed Luau that declares triggers and
//! caps, and calls steps from a closed catalog. Steps are a script's only
//! effects, and what a script cannot know (the date, what a step printed)
//! reaches it only as tokens, so [`Plan::build`] can show every path it can
//! take before anyone approves it, and [`Walker`] keeps real runs on it.
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
mod source;
mod step;
mod tokens;
mod vm;

pub use caps::Caps;
pub use error::{Budget, Error, Problem, Refusal, Result};
pub use plan::{Failure, MAX_PATHS, Plan, PlannedStep, TriggerPlan, Walker};
pub use schedule::{
    ClockTime, Cron, Days, Due, LATE_AFTER, Schedule, Zone, due, format_duration, parse_duration,
};
pub use script::{End, Script, Trigger};
pub use source::{MAX_SCRIPT_BYTES, Source};
pub use step::{Call, Kind, Step, Verdict};
pub use tokens::{DATE, Resolved, TIME, Values};
pub use vm::Limits;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
