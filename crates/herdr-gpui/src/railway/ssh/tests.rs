#![allow(clippy::unwrap_used)]
use super::*;
use std::fs;

fn text(identity: &str, known_hosts: &str) -> String {
    config_text(Path::new(identity), Path::new(known_hosts)).unwrap()
}

#[test]
fn the_config_offers_only_the_chosen_key_pins_the_host_and_forwards_nothing() {
    let config = text("/home/u/.ssh/id_ed25519", "/state/railway_known_hosts");
    for required in [
        "HostName railway.new",
        "AddressFamily inet",
        "IdentityFile \"/home/u/.ssh/id_ed25519\"",
        "IdentitiesOnly yes",
        "UserKnownHostsFile \"/state/railway_known_hosts\"",
        "GlobalKnownHostsFile /dev/null",
        "StrictHostKeyChecking accept-new",
        "BatchMode yes",
        "PasswordAuthentication no",
        "ForwardAgent no",
        "ClearAllForwardings yes",
        "ControlMaster no",
        "ProxyCommand none",
    ] {
        assert!(config.contains(required), "{required}");
    }
    assert!(!config.contains("StrictHostKeyChecking no"));
    // Every option stays inside the one host block.
    assert!(config.lines().skip(1).all(|line| line.starts_with('\t')));
}

#[test]
fn paths_cannot_end_the_value_or_start_a_token() {
    assert!(text("/a%d/b", "/h").contains("\"/a%%d/b\""));
    for bad in ["/a\"b", "/a\nHost x", "relative/key"] {
        assert!(
            config_text(Path::new(bad), Path::new("/h")).is_err(),
            "{bad:?}"
        );
        assert!(
            config_text(Path::new("/k"), Path::new(bad)).is_err(),
            "{bad:?}"
        );
    }
}

#[test]
fn the_config_is_private_and_outlives_every_command_built_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::new(Path::new("/k"), &dir.path().join("sub").join("known")).unwrap();
    let first = config.command();
    let args: Vec<_> = first
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(first.get_program(), "ssh");
    assert_eq!(args[0], "-F");
    assert_eq!(args[2..], ["-T", "--", ALIAS]);
    assert_eq!(config.terminal_command().get_args().nth(2).unwrap(), "-tt");
    let path = PathBuf::from(&args[1]);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("IdentityFile \"/k\"")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
    }
    assert!(
        dir.path().join("sub").is_dir(),
        "known_hosts can be written"
    );
    // The regression was a config dropped as soon as its command was handed out.
    drop(first);
    assert!(path.exists());
    drop(config);
    assert!(!path.exists());
}

/// OpenSSH itself must accept every option; `ssh -G` prints the result
/// without connecting anywhere.
#[test]
fn the_installed_ssh_resolves_the_config_as_intended() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::new(Path::new("/k/id"), &dir.path().join("known")).unwrap();
    let mut command = Command::new("ssh");
    command
        .args(config.command().get_args().take(2))
        .arg("-G")
        .arg(ALIAS);
    let Ok(output) = command.output() else {
        return; // ssh is not installed here.
    };
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let resolved = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "hostname railway.new",
        "identitiesonly yes",
        "identityfile /k/id",
        "forwardagent no",
        "addressfamily inet",
    ] {
        assert!(resolved.lines().any(|line| line == expected), "{expected}");
    }
}
