//! Helper for querying the local Azure CLI (`az`) when present. This lets users
//! and agents run without configuring a Service Principal or client secret.

use super::{Error, Result};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const SEARCH: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];

/// Find the `az` executable on PATH or in standard directories.
pub(crate) fn find_az() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AZURE_CLI_PATH").map(PathBuf::from)
        && executable(&path)
    {
        return Some(path);
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .chain(SEARCH.iter().map(PathBuf::from))
        .map(|dir| dir.join("az"))
        .find(|candidate| executable(candidate))
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.is_file()
}

pub(crate) fn is_az_available() -> bool {
    find_az().is_some()
}

fn run_az(args: &[&str]) -> Result<String> {
    let az_bin = find_az().ok_or(Error::AzMissing)?;
    let mut command = Command::new(az_bin);
    command.args(args);
    command.stdin(Stdio::null());
    crate::login_env::apply(&mut command);

    let output = command.output().map_err(Error::Process)?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(Error::AzFailed(err.trim().to_owned()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_owned())
}

/// Retrieve an access token for `audience` from `az`.
pub(crate) fn access_token(audience: super::http::Audience) -> Result<String> {
    run_az(&[
        "account",
        "get-access-token",
        "--resource",
        audience.resource(),
        "--query",
        "accessToken",
        "-o",
        "tsv",
    ])
}

/// Retrieve the currently active subscription ID from `az`.
pub(crate) fn default_subscription() -> Result<String> {
    run_az(&["account", "show", "--query", "id", "-o", "tsv"])
}

/// Retrieve the currently active tenant ID from `az`.
pub(crate) fn default_tenant() -> Result<String> {
    run_az(&["account", "show", "--query", "tenantId", "-o", "tsv"])
}
