//! Programs on this machine, found the way a host script's prelude finds
//! them, so a local host needs no shell and runs on every client platform.

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};

/// `program` in the user's tool directories under `home`, then on `path`,
/// the order the script prelude puts them in. On Windows a bare name also
/// matches its `.exe`, `.cmd`, or `.bat`.
pub(super) fn find(program: &str, home: Option<&Path>, path: Option<&OsStr>) -> Option<PathBuf> {
    let tools = home.into_iter().flat_map(|home| {
        [
            ".local/bin",
            ".local/share/mise/shims",
            ".bun/bin",
            ".cargo/bin",
        ]
        .map(|dir| home.join(dir))
    });
    let system = [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/home/linuxbrew/.linuxbrew/bin",
    ]
    .into_iter()
    .filter(|_| cfg!(unix))
    .map(PathBuf::from);
    let nix = home.map(|home| home.join(".nix-profile/bin"));
    let names: Vec<String> = if cfg!(windows) && Path::new(program).extension().is_none() {
        [".exe", ".cmd", ".bat"]
            .map(|extension| format!("{program}{extension}"))
            .into()
    } else {
        vec![program.to_owned()]
    };
    tools
        .chain(system)
        .chain(nix)
        .chain(path.into_iter().flat_map(std::env::split_paths))
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| runnable(candidate))
}

#[cfg(unix)]
fn runnable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn runnable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn tool(dir: &Path, name: &str) {
        std::fs::create_dir_all(dir).unwrap();
        let file = dir.join(name);
        std::fs::write(&file, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn user_tools_come_before_path() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let bin = dir.path().join("bin");
        let name = if cfg!(windows) { "bd.exe" } else { "bd" };
        tool(&home.join(".local/bin"), name);
        tool(&bin, name);
        let path = std::env::join_paths([&bin]).unwrap();
        let found = find("bd", Some(&home), Some(&path)).unwrap();
        assert!(found.starts_with(&home), "{found:?}");
        assert_eq!(find("bd", None, Some(&path)).unwrap(), bin.join(name));
        assert_eq!(find("absent", Some(&home), Some(&path)), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_that_is_not_executable_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bd"), "").unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert_eq!(find("bd", None, Some(&path)), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_command_script_is_found_by_its_bare_name() {
        let dir = tempfile::tempdir().unwrap();
        tool(dir.path(), "claude.cmd");
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert_eq!(
            find("claude", None, Some(&path)).unwrap(),
            dir.path().join("claude.cmd")
        );
    }
}
