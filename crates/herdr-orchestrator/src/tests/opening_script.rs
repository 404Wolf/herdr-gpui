//! Opening a project on an SSH host reads its repository, the agents the
//! host has, and the branches named here in one connection. The script runs
//! under a local shell, as on the host.
use super::*;
use crate::repo::{self, Extras};
use crate::test_executable;
use herdr_client::ConnectTarget;
use herdr_remote::teleport::Host;
use std::{path::Path, process::Command, sync::atomic::AtomicBool, time::Duration};

const PROGRAMS: [&str; 2] = ["claude", "codex"];

/// Runs the opening script for `checkout` with `bin` first on `PATH`.
fn open(dir: &Path, checkout: &Path, bin: &Path) -> Result<(repo::RepoInfo, Extras)> {
    let mut command = Command::new("/bin/sh");
    command
        .args([
            "-c",
            &repo::opening_script(&checkout.to_string_lossy(), &PROGRAMS),
        ])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", dir.join("gitconfig"));
    let output = Host::capture_command(command, Duration::from_secs(10), &AtomicBool::new(false))
        .map_err(error::script("Reading the repository"))?;
    let output = String::from_utf8(output).unwrap();
    let target = ConnectTarget::Ssh {
        target: "fixture".into(),
        session: "default".into(),
    };
    Ok((
        repo::parse(&target, &output)?,
        repo::extras(&output, &PROGRAMS),
    ))
}

fn git(checkout: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(checkout)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let checkout = dir.path().join("project with 'quotes'");
    std::fs::create_dir_all(&checkout).unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    test_executable::write(bin.join("claude"), "#!/bin/sh\n", 0o755).unwrap();
    (dir, checkout, bin)
}

#[test]
fn one_script_reads_the_repository_agents_and_branches() {
    let (dir, checkout, bin) = fixture();
    git(&checkout, &["init", "-q"]);
    git(
        &checkout,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/project.git",
        ],
    );
    git(&checkout, &["commit", "-q", "--allow-empty", "-m", "start"]);
    for branch in ["agent/377-icons-0123abcd", "agent/not-ours", "feature"] {
        git(&checkout, &["branch", branch]);
    }
    let (info, extras) = open(dir.path(), &checkout, &bin).unwrap();
    assert_eq!(info.remote.unwrap().source.repository, "example/project");
    assert_eq!(
        extras,
        Extras {
            programs: Some(vec!["claude".into()]),
            branches: Some(vec!["agent/377-icons-0123abcd".into()]),
        }
    );
    // The repository it reads is the one detection alone reads.
    let alone = Command::new("/bin/sh")
        .args(["-c", &repo::detection_script(&checkout.to_string_lossy())])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let target = ConnectTarget::Ssh {
        target: "fixture".into(),
        session: "default".into(),
    };
    let (opened, _) = open(dir.path(), &checkout, &bin).unwrap();
    assert_eq!(
        repo::parse(&target, &String::from_utf8(alone.stdout).unwrap()).unwrap(),
        opened
    );
}

#[test]
fn a_beads_folder_outside_git_still_reads_its_agents() {
    let (dir, checkout, bin) = fixture();
    std::fs::create_dir(checkout.join(".beads")).unwrap();
    let (info, extras) = open(dir.path(), &checkout, &bin).unwrap();
    assert!(info.beads);
    assert_eq!(extras.programs, Some(vec!["claude".into()]));
    assert_eq!(extras.branches, None, "no Git, so no branches were read");
}

#[test]
fn a_branch_read_that_fails_leaves_the_detection_and_reads_them_alone() {
    let (dir, checkout, bin) = fixture();
    git(&checkout, &["init", "-q"]);
    // A `git` that answers detection but fails to list branches.
    test_executable::write(
        bin.join("git"),
        "#!/bin/sh\nfor arg; do [ \"$arg\" = for-each-ref ] && exit 1; done\nexec /usr/bin/git \"$@\"\n",
        0o755,
    )
    .unwrap();
    let (info, extras) = open(dir.path(), &checkout, &bin).unwrap();
    assert!(info.remote.is_none());
    assert_eq!(extras.programs, Some(vec!["claude".into()]));
    assert_eq!(extras.branches, None);
}

#[test]
fn a_folder_outside_git_and_beads_is_still_skipped() {
    let (dir, checkout, bin) = fixture();
    assert!(matches!(
        open(dir.path(), &checkout, &bin),
        Err(Error::NotARepository)
    ));
}
