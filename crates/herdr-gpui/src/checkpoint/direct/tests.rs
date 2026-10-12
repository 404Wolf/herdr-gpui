#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::usage::Host;
use herdr_client::{ScriptHost, run_script};
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};

/// Scripts run with Git isolated from the user's configuration.
const ENV: &str = "GIT_CONFIG_GLOBAL=/dev/null\nGIT_CONFIG_NOSYSTEM=1\nexport GIT_CONFIG_GLOBAL GIT_CONFIG_NOSYSTEM\n";

fn isolated(_: &Checkout, body: &str, limits: ScriptLimits) -> Result<String> {
    let mut output = Vec::new();
    run_script(
        ScriptHost::Local,
        &format!("{ENV}{body}"),
        std::io::empty(),
        &mut output,
        limits,
        &AtomicBool::new(false),
    )
    .map_err(script::classify)?;
    Ok(String::from_utf8(output).unwrap())
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn the_command_line_takes_lists_and_diffs_checkpoints_as_the_app_does() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("main");
    fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    fs::write(main.join("edit.txt"), "one\n").unwrap();
    git(&main, &["add", "-A"]);
    git(&main, &["commit", "-q", "-m", "base"]);
    let key = git(&main, &["rev-parse", "--absolute-git-dir"]);
    let checkout = Checkout::new(Host::Local, key.trim(), Some("main")).unwrap();

    assert_eq!(list(&checkout, isolated).unwrap(), []);
    // A different size, so Git never takes the edit for a racily clean file.
    fs::write(main.join("edit.txt"), "second version\n").unwrap();
    let first = create(&checkout, "first", isolated).unwrap().unwrap();
    assert!(script::valid_id(&first) && first.len() == 16, "{first}");
    assert_eq!(
        create(&checkout, "again", isolated).unwrap(),
        None,
        "nothing changed since"
    );
    // Untracked work is part of a checkpoint and of the diff to the tree.
    fs::write(main.join("new.txt"), "fresh\n").unwrap();
    let patch = diff(&checkout, &first, None, isolated).unwrap();
    assert!(
        patch.contains("+++ b/new.txt") && patch.contains("+fresh"),
        "{patch}"
    );
    assert!(!patch.contains("edit.txt"), "{patch}");
    assert_eq!(
        git(&main, &["status", "--porcelain"]),
        " M edit.txt\n?? new.txt\n",
        "diffing leaves the index alone"
    );
    // A stamp is per millisecond; the next one must sort after the first.
    std::thread::sleep(std::time::Duration::from_millis(2));
    let second = create(&checkout, "second", isolated).unwrap().unwrap();
    let listed = list(&checkout, isolated).unwrap();
    let ids: Vec<_> = listed
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .collect();
    assert_eq!(ids, [second.as_str(), first.as_str()], "newest first");
    assert_eq!(listed[0].label, "second");
    assert_eq!(listed[0].branch.as_deref(), Some("main"));
    assert_eq!(listed[0].diff.files, 1);
    let between = diff(&checkout, &first, Some(&second), isolated).unwrap();
    assert!(between.contains("+++ b/new.txt"), "{between}");
    assert_eq!(diff(&checkout, &second, None, isolated).unwrap(), "");
    assert!(matches!(
        diff(&checkout, "0000000000000001", None, isolated),
        Err(Error::CheckpointMissing)
    ));
    assert!(matches!(
        diff(&checkout, "../heads/main", None, isolated),
        Err(Error::CheckpointMissing)
    ));
    let other = Checkout::new(Host::Local, key.trim(), Some("elsewhere")).unwrap();
    assert!(matches!(
        list(&other, isolated),
        Err(Error::CheckpointCheckout)
    ));
}
