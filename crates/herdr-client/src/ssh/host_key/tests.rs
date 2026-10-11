use super::*;

#[test]
fn fingerprints_are_read_from_ssh_keygen_lines() {
    assert_eq!(
        parse_fingerprint(
            "256 SHA256:eUXGGm1YGsMAS7vkcx6JOJdOGHPem5gQp4taiCfCLB8 gitlab.com (ED25519)"
        ),
        Some(HostKeyFingerprint {
            kind: "ED25519".into(),
            bits: 256,
            fingerprint: "SHA256:eUXGGm1YGsMAS7vkcx6JOJdOGHPem5gQp4taiCfCLB8".into(),
        })
    );
    // Hashed host names carry `|` and `=`; only the first and last words count.
    assert_eq!(
        parse_fingerprint("3072 SHA256:abc+/= |1|c2FsdA==|aGFzaA== (RSA)").map(|key| key.bits),
        Some(3072)
    );
    for line in [
        "",
        "256 MD5:aa:bb host (ED25519)",
        "x SHA256:abc host (ED25519)",
        "256 SHA256: host (ED25519)",
        "256 SHA256:a\u{1b}[31m host (ED25519)",
        "256 SHA256:abc host ED25519",
        "256 SHA256:abc host (ED 25519)",
    ] {
        assert_eq!(parse_fingerprint(line), None, "{line:?}");
    }
}

#[test]
fn known_hosts_files_come_from_ssh_g_in_order() {
    let output = b"hostname box\nuserknownhostsfile /home/me/.ssh/known_hosts /home/me/.ssh/known_hosts2\nport 22\n";
    assert_eq!(
        parse_known_hosts(output),
        Some(vec![
            PathBuf::from("/home/me/.ssh/known_hosts"),
            PathBuf::from("/home/me/.ssh/known_hosts2"),
        ])
    );
    for output in [
        &b"hostname box\n"[..],
        b"userknownhostsfile none\n",
        b"userknownhostsfile ~/.ssh/known_hosts\n",
        // A path with a space reaches `ssh -G` output as two words.
        b"userknownhostsfile /home/my files/known_hosts\n",
        b"userknownhostsfile \"/home/me/known_hosts\"\n",
    ] {
        assert_eq!(parse_known_hosts(output), None);
    }
}

#[test]
fn scan_options_record_into_scratch_and_offer_no_credential() {
    let options = scan_options(
        Path::new("/tmp/scratch/known_hosts"),
        &[PathBuf::from("/home/me/.ssh/known_hosts")],
    );
    assert_eq!(options.len(), 18);
    assert!(options.iter().step_by(2).all(|flag| flag == "-o"));
    let values: Vec<&str> = options
        .iter()
        .skip(1)
        .step_by(2)
        .map(String::as_str)
        .collect();
    assert_eq!(
        values[0],
        "UserKnownHostsFile=\"/tmp/scratch/known_hosts\" \"/home/me/.ssh/known_hosts\""
    );
    for expected in [
        "StrictHostKeyChecking=accept-new",
        "PubkeyAuthentication=no",
        "PasswordAuthentication=no",
        "KbdInteractiveAuthentication=no",
        "ControlPath=none",
    ] {
        assert!(values.contains(&expected), "{expected}");
    }
}

#[test]
fn entries_with_controls_are_refused() {
    assert_eq!(
        valid_entries("host ssh-ed25519 AAAA\n\n").unwrap(),
        "host ssh-ed25519 AAAA\n"
    );
    assert!(matches!(
        valid_entries("host ssh-ed25519 AAAA\u{1b}]52;c;x\u{7}"),
        Err(Error::RemoteOutput)
    ));
}

fn key(known_hosts: PathBuf) -> NewHostKey {
    NewHostKey {
        fingerprints: Vec::new(),
        entries: "box ssh-ed25519 AAAA\n".into(),
        known_hosts,
    }
}

#[test]
fn trusting_appends_after_an_unterminated_last_line() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("known_hosts");
    std::fs::write(&path, "other ssh-rsa BBBB").unwrap();
    trust_host_key(&key(path.clone())).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "other ssh-rsa BBBB\nbox ssh-ed25519 AAAA\n"
    );
}

#[test]
fn trusting_creates_a_private_directory_and_file() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(".ssh").join("known_hosts");
    trust_host_key(&key(path.clone())).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "box ssh-ed25519 AAAA\n"
    );
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(path.parent().unwrap()), 0o700);
    assert_eq!(mode(&path), 0o600);
}

#[test]
fn a_failed_append_names_the_operation() {
    let root = tempfile::tempdir().unwrap();
    // A directory where the file should be cannot be opened for appending.
    let path = root.path().join("known_hosts");
    std::fs::create_dir(&path).unwrap();
    let error = trust_host_key(&key(path.clone())).unwrap_err();
    assert!(matches!(
        &error,
        Error::Storage { operation: StorageOperation::Open, path: failed, .. } if failed == &path
    ));
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn invalid_targets_are_refused_before_ssh_runs() {
    assert!(matches!(
        scan_host_key("-oProxyCommand=x"),
        Err(Error::InvalidSshTarget)
    ));
}
