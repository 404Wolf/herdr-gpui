//! The plan a person reviews before approving a script. Steps are a
//! script's only effects, so running each trigger with faked outcomes shows
//! everything it can do: once with every step succeeding, then once per
//! step with that one step failing.

use crate::{
    Caps, Result,
    schedule::{Schedule, Zone},
    script::{End, Fire, Reason, Script},
    step::{Call, Kind, Outcome},
};
use serde::Serialize;
use std::fmt;

/// What a faked failing step shows the script as its log.
pub const FAKE_LOG_TAIL: &str = "<log tail of the failed step>";

#[derive(Debug, Serialize)]
pub struct Plan {
    pub script: String,
    pub caps: Caps,
    pub triggers: Vec<TriggerPlan>,
}

#[derive(Debug, Serialize)]
pub struct TriggerPlan {
    pub name: Option<String>,
    pub schedule: Schedule,
    pub zone: Zone,
    /// The steps when every one succeeds.
    pub steps: Vec<PlannedStep>,
}

#[derive(Debug, Serialize)]
pub struct PlannedStep {
    #[serde(flatten)]
    pub call: Call,
    pub on_failure: Failure,
}

/// What the run does when a step fails, all others succeeding.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "then", content = "steps")]
pub enum Failure {
    /// The step was checked: the run stops there.
    Stops,
    /// The script reads the failure but goes on with the same steps.
    Continues,
    /// The script takes other steps, possibly none.
    Branch(Vec<Call>),
}

impl Plan {
    pub fn of(script: &Script) -> Result<Self> {
        let mut triggers = Vec::with_capacity(script.triggers().len());
        for (index, trigger) in script.triggers().iter().enumerate() {
            let fire = Fire {
                reason: match trigger.schedule {
                    Schedule::Manual => Reason::Manual,
                    _ => Reason::Schedule,
                },
                date: "<date>".to_owned(),
                time: match &trigger.schedule {
                    Schedule::At { time, .. } => time.to_string(),
                    _ => "<time>".to_owned(),
                },
            };
            let (success, _) = simulate(script, index, &fire, None)?;
            let mut steps = Vec::with_capacity(success.len());
            for (position, call) in success.iter().enumerate() {
                let (failed, end) = simulate(script, index, &fire, Some(position))?;
                let rest = failed.get(position + 1..).unwrap_or_default();
                let on_failure = match end {
                    End::Stopped { .. } => Failure::Stops,
                    End::Completed if Some(rest) == success.get(position + 1..) => {
                        Failure::Continues
                    }
                    End::Completed => Failure::Branch(rest.to_vec()),
                };
                steps.push(PlannedStep {
                    call: call.clone(),
                    on_failure,
                });
            }
            triggers.push(TriggerPlan {
                name: trigger.name.clone(),
                schedule: trigger.schedule.clone(),
                zone: trigger.zone.clone(),
                steps,
            });
        }
        Ok(Self {
            script: script.name().to_owned(),
            caps: script.caps().clone(),
            triggers,
        })
    }
}

/// Runs a trigger with every step succeeding but the one at `fail_at`.
fn simulate(
    script: &Script,
    index: usize,
    fire: &Fire,
    fail_at: Option<usize>,
) -> Result<(Vec<Call>, End)> {
    let mut calls = Vec::new();
    let end = script.run(index, fire, |call| {
        let failing = fail_at == Some(calls.len());
        calls.push(call.clone());
        fake_outcome(call, failing)
    })?;
    Ok((calls, end))
}

fn fake_outcome(call: &Call, failing: bool) -> Outcome {
    if failing {
        return Outcome {
            ok: false,
            exit: Some(1),
            log_tail: FAKE_LOG_TAIL.to_owned(),
            ..Outcome::default()
        };
    }
    Outcome {
        ok: true,
        exit: Some(0),
        run_id: (call.step.kind() == Kind::Agent).then(|| "<run>".to_owned()),
        ..Outcome::default()
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.script)?;
        writeln!(f, "  caps: {}", self.caps)?;
        for trigger in &self.triggers {
            f.write_str("  ")?;
            if let Some(name) = &trigger.name {
                write!(f, "{name}: ")?;
            }
            write!(f, "{}", trigger.schedule)?;
            if matches!(
                trigger.schedule,
                Schedule::At { .. } | Schedule::Cron { .. }
            ) {
                write!(f, " ({})", trigger.zone)?;
            }
            writeln!(f)?;
            if trigger.steps.is_empty() {
                writeln!(f, "    (no steps)")?;
            }
            for (index, step) in trigger.steps.iter().enumerate() {
                writeln!(f, "    {}. {}", index + 1, step.call)?;
                match &step.on_failure {
                    Failure::Stops => {}
                    Failure::Continues => {
                        writeln!(f, "       └ on failure: continues with the steps below")?
                    }
                    Failure::Branch(steps) if steps.is_empty() => {
                        writeln!(f, "       └ on failure: the run ends")?;
                    }
                    Failure::Branch(steps) => {
                        writeln!(f, "       └ on failure, instead:")?;
                        for call in steps {
                            writeln!(f, "           - {call}")?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
