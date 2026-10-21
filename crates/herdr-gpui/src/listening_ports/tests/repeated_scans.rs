use crate::listening_ports::{STEP_TIMEOUT, local_shell, scan::COMMAND};

/// The step runs again and again in one long-lived shell, so after the first
/// run it must leave `PATH` as it found it rather than grow it every scan.
#[test]
fn repeated_scans_keep_path_the_same() {
    let mut shell = local_shell().unwrap();
    let mut path = || {
        shell.run(COMMAND, STEP_TIMEOUT).unwrap();
        shell
            .run("printf '%s\\n' \"$PATH\"", STEP_TIMEOUT)
            .unwrap()
            .stdout
    };
    let first = path();
    for _ in 0..3 {
        assert_eq!(path(), first);
    }
    let entries: Vec<&str> = first.trim_end().split(':').collect();
    for directory in ["/usr/sbin", "/sbin"] {
        assert!(
            entries.contains(&directory),
            "{directory} missing from {first:?}"
        );
    }
}
