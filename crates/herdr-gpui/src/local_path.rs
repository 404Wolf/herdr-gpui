use std::{ffi::OsString, path::PathBuf};

/// GUI launches omit directories commonly used by user-installed tools.
pub(crate) fn local_path() -> OsString {
    let mut dirs = Vec::new();
    if let Ok(home) = crate::config::home() {
        dirs.extend(
            [".local/bin", ".cargo/bin", ".bun/bin", ".npm-global/bin"].map(|dir| home.join(dir)),
        );
    }
    if cfg!(unix) {
        dirs.extend(
            [
                "/home/linuxbrew/.linuxbrew/bin",
                "/opt/homebrew/bin",
                "/usr/local/bin",
            ]
            .map(PathBuf::from),
        );
    }
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    dirs.extend(std::env::split_paths(&inherited));
    // launchd's `PATH` stops at `/usr/bin:/bin`; system tools such as `lsof`,
    // which the Tailscale CLI runs on macOS, live in the `sbin` directories.
    if cfg!(unix) {
        for system in ["/usr/sbin", "/sbin"].map(PathBuf::from) {
            if !dirs.contains(&system) {
                dirs.push(system);
            }
        }
    }
    std::env::join_paths(dirs).unwrap_or(inherited)
}

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn fallback_includes_homebrew() {
        let paths: Vec<_> = std::env::split_paths(&super::local_path()).collect();
        assert!(
            paths
                .iter()
                .any(|path| path == std::path::Path::new("/opt/homebrew/bin"))
        );
        assert!(
            paths
                .iter()
                .any(|path| path == std::path::Path::new("/home/linuxbrew/.linuxbrew/bin"))
        );
    }

    #[test]
    fn fallback_includes_system_tools_once() {
        let path: Vec<_> = std::env::split_paths(&super::local_path()).collect();
        for system in ["/usr/sbin", "/sbin"] {
            let system = std::path::Path::new(system);
            assert_eq!(path.iter().filter(|dir| *dir == system).count(), 1);
        }
    }
}
