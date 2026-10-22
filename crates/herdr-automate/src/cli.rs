//! The command line: parsing from OS strings, so a script's path need not
//! be UTF-8, and running the parsed command against any writer.

use herdr_automation::{Kind, Limits, Plan, Source};
use serde_json::json;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;

pub const HELP: &str = "\
herdr-automate: scheduled and manual scripts for Herdr

Usage:
  herdr-automate check FILE [--json]   Load a script and print its plan, or what is wrong
  herdr-automate steps [--json]        List the steps a script can call
  herdr-automate --help | --version

A script is a Luau file, usually .herdr/automations/NAME.luau. Its top level
declares caps and triggers; each trigger's function calls steps, in order.
Steps are a script's only effects: there is no clock, randomness, file,
network, process or require, so the plan `check` prints is everything the
script can do.

  herdr.caps { agents_per_run = 1, tokens_per_day = 2_000_000, wall_time = \"3h\" }

  herdr.at(\"03:00\", { days = \"mon-fri\", tz = \"Europe/Paris\" }, function(fire) ... end)
  herdr.every(\"6h\", function(fire) ... end)
  herdr.cron(\"*/15 9-17 * * mon-fri\", function(fire) ... end)
  herdr.manual({ name = \"deploy\" }, function(fire) ... end)

Every trigger can also be started by hand; herdr.manual has no schedule.
Triggers take an optional { name = \"...\" }.

A step that fails stops the run, unless called with { check = false }: then
it returns { ok, log_tail } (and run_id for agent) and the script decides.
Steps also take { timeout = \"30m\" }. `herdr-automate steps` lists them.

What a script cannot know reaches it as tokens it can pass along but not
read: fire.date is \"{date}\", fire.time is \"{time}\", and a result's log_tail
is \"{log_tail:N}\" for step N. The runner fills them in when it runs a step;
in a sh command they become \"$HERDR_...\" variables. So the plan, which tries
every combination of unchecked results, is every path the script can take,
and a run that leaves it is halted.

Exit status: 0 the script is valid, 1 it is not or cannot be read, 2 usage.
";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Check { file: PathBuf, json: bool },
    Help,
    Steps { json: bool },
    Version,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("`check` needs a script file")]
    MissingFile,
    #[error("no command given; try `herdr-automate --help`")]
    NoCommand,
    #[error("unexpected argument `{0}`")]
    Unexpected(String),
    #[error("unknown command `{0}`; try `herdr-automate --help`")]
    UnknownCommand(String),
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Command, Error> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Err(Error::NoCommand);
    };
    let lossy = |arg: &OsString| arg.to_string_lossy().into_owned();
    match command.to_str() {
        Some("-h" | "--help" | "help") => no_more(args).map(|()| Command::Help),
        Some("-V" | "--version") => no_more(args).map(|()| Command::Version),
        Some("check") => {
            let mut file = None;
            let mut json = false;
            for arg in args {
                match arg.to_str() {
                    Some("--json") => json = true,
                    _ if file.is_none() && !arg.to_string_lossy().starts_with("--") => {
                        file = Some(PathBuf::from(arg));
                    }
                    _ => return Err(Error::Unexpected(lossy(&arg))),
                }
            }
            Ok(Command::Check {
                file: file.ok_or(Error::MissingFile)?,
                json,
            })
        }
        Some("steps") => {
            let mut json = false;
            for arg in args {
                match arg.to_str() {
                    Some("--json") => json = true,
                    _ => return Err(Error::Unexpected(lossy(&arg))),
                }
            }
            Ok(Command::Steps { json })
        }
        _ => Err(Error::UnknownCommand(lossy(&command))),
    }
}

fn no_more(mut args: impl Iterator<Item = OsString>) -> Result<(), Error> {
    match args.next() {
        Some(arg) => Err(Error::Unexpected(arg.to_string_lossy().into_owned())),
        None => Ok(()),
    }
}

/// The version CI embeds in release builds.
pub fn version() -> &'static str {
    option_env!("HERDR_RELEASE_VERSION").unwrap_or("dev")
}

/// Runs a command, writing its answer to `out`.
pub fn run(command: &Command, out: &mut impl Write) -> io::Result<ExitCode> {
    match command {
        Command::Help => {
            out.write_all(HELP.as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Version => {
            writeln!(out, "herdr-automate {}", version())?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Steps { json } => {
            if *json {
                let steps = Kind::ALL
                    .iter()
                    .map(|kind| json!({ "name": kind.name(), "usage": kind.usage(), "summary": kind.summary() }))
                    .collect::<Vec<_>>();
                writeln!(out, "{}", json!({ "steps": steps }))?;
            } else {
                for kind in Kind::ALL {
                    writeln!(
                        out,
                        "{}\n    {}\n    {}",
                        kind.name(),
                        kind.usage(),
                        kind.summary()
                    )?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Check { file, json } => check(file, *json, out),
    }
}

fn check(file: &PathBuf, json: bool, out: &mut impl Write) -> io::Result<ExitCode> {
    let plan = Source::read(file).and_then(|source| Plan::build(&source, Limits::default()));
    match (plan, json) {
        (Ok(plan), false) => {
            write!(out, "{plan}")?;
            let count = plan.triggers.len();
            writeln!(
                out,
                "\nOK: {count} trigger{}. Every trigger can also be started by hand.",
                if count == 1 { "" } else { "s" }
            )?;
            Ok(ExitCode::SUCCESS)
        }
        (Ok(plan), true) => {
            writeln!(out, "{}", json!({ "ok": true, "plan": plan }))?;
            Ok(ExitCode::SUCCESS)
        }
        (Err(error), false) => {
            writeln!(out, "error: {error}")?;
            Ok(ExitCode::FAILURE)
        }
        (Err(error), true) => {
            let budget = error.budget();
            writeln!(
                out,
                "{}",
                json!({ "ok": false, "error": error.to_string(), "budget": budget })
            )?;
            Ok(ExitCode::FAILURE)
        }
    }
}
