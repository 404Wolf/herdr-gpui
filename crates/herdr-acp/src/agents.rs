//! How `herdr-acp` finds the command that starts an agent; the agents
//! themselves are `herdr_chat_protocol::Agent`, shared with its clients.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

pub(crate) use herdr_chat_protocol::Agent;

/// The first of `agent`'s commands found on `path`, or the last one by name
/// so that starting it fails with a message naming what to install.
pub(crate) fn command(agent: Agent, path: Option<&OsStr>) -> (OsString, Vec<OsString>) {
    let candidates = agent.candidates();
    let found = candidates
        .iter()
        .find_map(|(program, args)| find_program(program, path).map(|found| (found, *args)));
    let (program, args) = match found {
        Some((found, args)) => (found.into_os_string(), args),
        None => {
            let (program, args) = candidates[candidates.len() - 1];
            (OsString::from(program), args)
        }
    };
    (program, args.iter().map(OsString::from).collect())
}

/// Looks `program` up in a `PATH`-style list of directories.
pub(crate) fn find_program(program: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    let path = path?;
    std::env::split_paths(path).find_map(|dir| executable(&dir, program))
}

#[cfg(unix)]
fn executable(dir: &Path, program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let candidate = dir.join(program);
    let metadata = candidate.metadata().ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(candidate)
}

#[cfg(windows)]
fn executable(dir: &Path, program: &str) -> Option<PathBuf> {
    ["exe", "cmd", "bat"]
        .iter()
        .map(|extension| dir.join(program).with_extension(extension))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests;
