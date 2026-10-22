//! A loaded script: its caps and triggers, read once at load, and a way to
//! run one trigger with a caller's step executor. The planner passes a fake
//! executor; the runner passes one that does the work.

use crate::{
    Caps, Error, Refusal, Result,
    step::{Call, Kind, Outcome},
    vm::{self, Limits, Meter},
};
use mlua::{Function, Lua, MultiValue, Value};
use serde::Serialize;
use std::{cell::RefCell, fmt, fs, io::Read, path::Path, rc::Rc};

mod declare;

pub use declare::Trigger;

/// The largest script file read.
pub const MAX_SCRIPT_BYTES: u64 = 256 * 1024;

/// The registry slot holding the current run's step dispatcher.
const DISPATCH: &str = "herdr.dispatch";

pub struct Script {
    name: String,
    lua: Lua,
    meter: Meter,
    limits: Limits,
    caps: Caps,
    triggers: Vec<Trigger>,
}

impl fmt::Debug for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Script")
            .field("name", &self.name)
            .field("caps", &self.caps)
            .field("triggers", &self.triggers)
            .finish_non_exhaustive()
    }
}

/// Why a trigger fired, as the script sees it in `fire.reason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    /// Its schedule came round.
    Schedule,
    /// Its schedule came round while the runner was down.
    Late,
    /// Someone started it.
    Manual,
}

impl Reason {
    fn name(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Late => "late",
            Self::Manual => "manual",
        }
    }
}

/// What a trigger's handler receives. The runner fills it from the clock;
/// the script never reads a clock itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fire {
    pub reason: Reason,
    /// `YYYY-MM-DD` in the trigger's zone.
    pub date: String,
    /// `HH:MM` in the trigger's zone.
    pub time: String,
}

/// How a run ended without an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    Completed,
    /// A checked step failed; nothing after it ran.
    Stopped {
        step: usize,
    },
}

impl Script {
    /// Reads and loads a script file, named after its file stem.
    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let read = |source| Error::Read {
            path: path.to_owned(),
            source,
        };
        let file = fs::File::open(path).map_err(read)?;
        let size = file.metadata().map_err(read)?.len();
        if size > MAX_SCRIPT_BYTES {
            return Err(Error::TooLarge {
                size,
                limit: MAX_SCRIPT_BYTES,
            });
        }
        let mut source = String::new();
        file.take(MAX_SCRIPT_BYTES + 1)
            .read_to_string(&mut source)
            .map_err(read)?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "script".to_owned());
        Self::load(name, &source, Limits::default())
    }

    /// Loads a script from its source. Its top level runs once, declaring
    /// caps and triggers; nothing else may happen there.
    pub fn load(name: impl Into<String>, source: &str, limits: Limits) -> Result<Self> {
        let name = name.into();
        let meter = Meter::default();
        let lua = vm::open(limits, &meter)?;
        let declared = Rc::new(RefCell::new(declare::Declared::default()));
        declare::install(&lua, &declared)?;
        install_steps(&lua)?;
        vm::seal(&lua)?;
        meter.reset();
        lua.load(source).set_name(format!("={name}")).exec()?;
        let declared = declared.replace(declare::Declared::closed());
        let caps = declared.caps.ok_or(Error::MissingCaps)?;
        if declared.triggers.is_empty() {
            return Err(Error::NoTriggers);
        }
        Ok(Self {
            name,
            lua,
            meter,
            limits,
            caps,
            triggers: declared.triggers,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn caps(&self) -> &Caps {
        &self.caps
    }

    pub fn triggers(&self) -> &[Trigger] {
        &self.triggers
    }

    /// Runs trigger `index`, handing each step to `exec` in order. A checked
    /// step whose outcome is not `ok` stops the run.
    pub fn run(
        &self,
        index: usize,
        fire: &Fire,
        mut exec: impl FnMut(&Call) -> Outcome,
    ) -> Result<End> {
        let trigger = self
            .triggers
            .get(index)
            .ok_or(Error::UnknownTrigger(index))?;
        let fire_table = self.lua.create_table()?;
        fire_table.raw_set("reason", fire.reason.name())?;
        fire_table.raw_set("date", fire.date.as_str())?;
        fire_table.raw_set("time", fire.time.as_str())?;
        self.meter.reset();
        let steps = self.limits.steps;
        let result = self.lua.scope(|scope| {
            let mut count = 0usize;
            let dispatch = scope.create_function_mut(move |lua, mut args: MultiValue| {
                let index = match args.pop_front() {
                    Some(Value::Integer(index)) => usize::try_from(index).ok(),
                    Some(Value::Number(index)) if index.fract() == 0. && index >= 0. => {
                        Some(index as usize)
                    }
                    _ => None,
                };
                let kind = index
                    .and_then(|index| Kind::ALL.get(index).copied())
                    .ok_or_else(|| mlua::Error::runtime("unknown step"))?;
                let call = Call::read(kind, args)?;
                if count >= steps {
                    return Err(mlua::Error::external(Refusal::Budget(crate::Budget::Steps)));
                }
                let step = count;
                count += 1;
                let outcome = exec(&call);
                if call.check && !outcome.ok {
                    return Err(mlua::Error::external(Refusal::StepFailed(step)));
                }
                outcome.to_table(lua)
            })?;
            self.lua.set_named_registry_value(DISPATCH, dispatch)?;
            let result = trigger.handler().call::<()>(fire_table);
            self.lua.unset_named_registry_value(DISPATCH)?;
            Ok(result)
        })?;
        match result {
            Ok(()) => Ok(End::Completed),
            Err(error) => match error.downcast_ref::<Refusal>() {
                Some(Refusal::StepFailed(step)) => Ok(End::Stopped { step: *step }),
                _ => Err(Error::Lua(error)),
            },
        }
    }
}

/// A global per step that forwards to the current run's dispatcher, or
/// refuses when no run is going on (at the script's top level).
fn install_steps(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    for (index, kind) in Kind::ALL.iter().copied().enumerate() {
        let index = i64::try_from(index).map_err(mlua::Error::external)?;
        let step = lua.create_function(move |lua, mut args: MultiValue| {
            let Some(dispatch) = lua.named_registry_value::<Option<Function>>(DISPATCH)? else {
                return Err(mlua::Error::external(Refusal::StepOutsideTrigger(
                    kind.name(),
                )));
            };
            args.push_front(Value::Integer(index));
            dispatch.call::<Value>(args)
        })?;
        globals.raw_set(kind.name(), step)?;
    }
    Ok(())
}
