//! How `herdr-acp` finds the command that starts an agent; the agents
//! themselves are `herdr_chat_protocol::Agent`, shared with its clients.

use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

pub(crate) use herdr_chat_protocol::Agent;

/// Where agents install themselves besides `PATH`, relative to the home
/// folder, searched after it: a noninteractive SSH shell, or an app started
/// from the desktop, often runs with a `PATH` that leaves them out.
const USER_DIRS: [&str; 6] = [
    ".local/bin",
    ".cargo/bin",
    ".bun/bin",
    ".npm-global/bin",
    ".opencode/bin",
    ".volta/bin",
];
/// System folders package managers install into, searched last.
#[cfg(unix)]
const SYSTEM_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];
#[cfg(not(unix))]
const SYSTEM_DIRS: [&str; 0] = [];
/// The runner that fetches a package on first use; an agent only it can
/// start is not installed.
const FETCHER: &str = "npx";

/// `path`'s folders, then the user and system folders agents install to.
pub(crate) fn search_path(path: Option<&OsStr>, home: Option<&Path>) -> OsString {
    let mut dirs: Vec<PathBuf> = path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .collect();
    dirs.extend(
        home.into_iter()
            .flat_map(|home| USER_DIRS.iter().map(|dir| home.join(dir))),
    );
    dirs.extend(SYSTEM_DIRS.iter().map(PathBuf::from));
    std::env::join_paths(dirs).unwrap_or_default()
}

/// The agents whose own ACP command is on `path`, in table order. One only
/// `npx` could start does not count: it would be downloaded, not run.
pub(crate) fn installed(path: Option<&OsStr>) -> Vec<Agent> {
    Agent::ALL
        .iter()
        .copied()
        .filter(|agent| {
            agent
                .candidates()
                .iter()
                .filter(|(program, _)| *program != FETCHER)
                .any(|(program, _)| find_program(program, path).is_some())
        })
        .collect()
}

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
