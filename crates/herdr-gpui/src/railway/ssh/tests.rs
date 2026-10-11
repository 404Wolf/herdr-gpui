#![allow(clippy::unwrap_used)]
use super::*;
use std::fs;

fn text(identity: &str, known_hosts: &str) -> String {
    config_text(Path::new(identity), Path::new(known_hosts)).unwrap()
}

#[test]
fn the_config_offers_only_the_chosen_key_to_railway() {
    let config = text("/home/u/.ssh/id_ed25519", "/state/railway_known_hosts");
    assert!(config.contains("HostName railway.new"));
    assert!(config.contains("IdentityFile \"/home/u/.ssh/id_ed25519\""));
    assert!(config.contains("IdentitiesOnly yes"));
    assert!(config.contains("UserKnownHostsFile \"/state/railway_known_hosts\""));
    assert!(config.contains("GlobalKnownHostsFile /dev/null"));
    assert!(config.contains("StrictHostKeyChecking accept-new"));
    assert!(
        config.contains("AddressFamily inet"),
        "Railway is IPv4 only"
    );
    for line in config.lines().skip(1) {
        assert!(
            line.starts_with('\t'),
            "every option stays in the host block"
        );
    }
}

#[test]
fn the_config_is_isolated_noninteractive_and_forwards_nothing() {
    let config = text("/k", "/h");
    for required in [
        "BatchMode yes",
        "NumberOfPasswordPrompts 0",
        "PreferredAuthentications publickey",
        "PasswordAuthentication no",
        "ForwardAgent no",
        "ForwardX11 no",
        "ClearAllForwardings yes",
        "ControlMaster no",
        "ControlPath none",
        "ProxyCommand none",
        "PermitLocalCommand no",
        "ConnectTimeout 15",
        "ServerAliveInterval 15",
    ] {
        assert!(config.contains(required), "{required}");
    }
    assert!(!config.contains("StrictHostKeyChecking no"));
    assert!(
        !config.contains("User "),
        "the user stays what `ssh railway.new` uses"
    );
    assert!(!config.contains("RemoteForward") && !config.contains("LocalForward"));
}

#[test]
fn paths_cannot_end_the_value_or_start_a_token() {
    assert!(text("/a%d/b", "/h").contains("\"/a%%d/b\""));
    for bad in ["/a\"b", "/a\nHost x", "/a\rb", "relative/key", "key"] {
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
fn the_command_reads_a_private_config_that_outlives_every_command_built_from_it() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::new(Path::new("/k"), &dir.path().join("sub").join("known")).unwrap();
    let first = config.command();
    let second = config.command();
    let args: Vec<_> = first
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(first.get_program(), "ssh");
    assert_eq!(&args[0], "-F");
    assert_eq!(&args[2..], ["-T", "--", ALIAS]);
    assert_eq!(first.get_args().nth(1), second.get_args().nth(1));
    let path = PathBuf::from(&args[1]);
    // Commands exist (and may be spawned) while the config does; the
    // regression was a config dropped as soon as its command was handed out.
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
    drop(first);
    assert!(
        path.exists(),
        "dropping a command does not remove the config"
    );
    drop(config);
    assert!(!path.exists());
}

#[test]
fn nothing_secret_or_remote_reaches_the_argument_list() {
    let config = Config::new(Path::new("/very/private/key"), Path::new("/h")).unwrap();
    for arg in config.command().get_args() {
        assert!(!arg.to_string_lossy().contains("private"), "{arg:?}");
    }
}

/// OpenSSH itself must accept every option and resolve them as intended;
/// `ssh -G` prints the result without connecting anywhere.
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
        return; // ssh is not installed here; the option names are unchecked.
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "ssh rejected the config: {stderr}");
    let resolved = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "hostname railway.new",
        "port 22",
        "identitiesonly yes",
        "identityfile /k/id",
        "batchmode yes",
        "forwardagent no",
        "clearallforwardings yes",
        "addressfamily inet",
        "stricthostkeychecking accept-new",
        "passwordauthentication no",
    ] {
        assert!(
            resolved.lines().any(|line| line == expected),
            "{expected} missing from:\n{resolved}"
        );
    }
    assert!(resolved.contains(&format!(
        "userknownhostsfile {}",
        dir.path().join("known").display()
    )));
}
