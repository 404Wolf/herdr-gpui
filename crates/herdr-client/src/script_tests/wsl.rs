#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use std::io;

const LIMITS: ScriptLimits = ScriptLimits {
    output: 1 << 20,
    idle: Duration::from_secs(30),
};

fn wsl(distro: &str, script: &str, input: &[u8], cancelled: bool) -> (Result<u64>, Vec<u8>) {
    let mut output = Vec::new();
    let result = run_script(
        ScriptHost::Wsl(distro),
        script,
        input,
        &mut output,
        LIMITS,
        &AtomicBool::new(cancelled),
    );
    (result, output)
}

#[test]
fn a_name_that_is_not_a_distribution_is_refused_first() {
    for name in ["", "-d", "two words", "a;b"] {
        let (result, _) = wsl(name, "true", b"", false);
        assert!(matches!(result, Err(Error::InvalidWslDistro)), "{name}");
    }
}

#[cfg(not(windows))]
#[test]
fn a_distribution_needs_a_windows_client() {
    let (result, _) = wsl("Ubuntu", "true", b"", false);
    assert!(matches!(result, Err(Error::WslUnsupported)));
}

#[cfg(windows)]
#[test]
fn a_windows_client_runs_scripts_only_in_distributions() {
    for host in [ScriptHost::Local, ScriptHost::Ssh("me@box")] {
        let result = run_script(
            host,
            "true",
            io::empty(),
            io::sink(),
            LIMITS,
            &AtomicBool::new(false),
        );
        assert!(matches!(result, Err(Error::ScriptUnsupported)), "{host:?}");
    }
}

#[cfg(windows)]
#[test]
fn cancellation_before_start_never_starts_wsl() {
    let (result, _) = wsl("Ubuntu", "true", b"", true);
    assert!(matches!(result, Err(Error::ScriptCancelled)));
}

#[cfg(windows)]
#[test]
fn a_distribution_script_is_one_argument_after_exec() {
    let script = "printf '%s\\n' \"$HOME\" `x` \"q\"";
    let command = crate::wsl::command("Ubuntu-24.04", script);
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(
        args,
        [
            "--distribution",
            "Ubuntu-24.04",
            "--cd",
            "~",
            "--exec",
            "/bin/sh",
            "-c",
            script
        ]
    );
}

/// Runs scripts in a real distribution:
/// `HERDR_TEST_WSL_DISTRO=Ubuntu-22.04 cargo test -p herdr-client
/// live_wsl_scripts -- --ignored --nocapture`.
#[cfg(windows)]
#[test]
#[ignore = "runs scripts in a real WSL distribution"]
fn live_wsl_scripts() {
    let distro = std::env::var("HERDR_TEST_WSL_DISTRO").expect("HERDR_TEST_WSL_DISTRO");
    // Quotes, `$`, backquotes, and newlines reach the distribution's shell as
    // written, and binary input streams through unchanged.
    let input: Vec<u8> = (0..=255).cycle().take(200_000).collect();
    let (result, output) = wsl(
        &distro,
        "cat\nprintf '%s' \"$PWD\" '`$x`' \"q'\"",
        &input,
        false,
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(&output[..input.len()], input.as_slice());
    let tail = String::from_utf8(output[input.len()..].to_vec()).unwrap();
    assert!(tail.ends_with("`$x`q'"), "{tail}");
    assert!(tail.starts_with('/'), "{tail}");

    let (result, _) = wsl(&distro, "printf 'failure\\033' >&2; exit 7", b"", false);
    assert!(
        matches!(&result, Err(Error::ScriptExit { status, stderr }) if status.code() == Some(7) && stderr == "failure"),
        "{result:?}"
    );

    let mut output = Vec::new();
    let result = run_script(
        ScriptHost::Wsl(&distro),
        "head -c 4096 /dev/zero",
        io::empty(),
        &mut output,
        ScriptLimits {
            output: 1024,
            idle: Duration::from_secs(30),
        },
        &AtomicBool::new(false),
    );
    assert!(
        matches!(result, Err(Error::ScriptOutputLimit)),
        "{result:?}"
    );
}
