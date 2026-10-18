//! gix reads local repositories as the `git` commands it replaced print
//! them, checked against `git` itself.

use crate::orchestrator::git;
use std::{path::Path, process::Command};

fn run(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}

/// A repository with one commit at `dir/app`.
fn repository(dir: &Path) -> std::path::PathBuf {
    let main = dir.join("app");
    std::fs::create_dir_all(&main).unwrap();
    run(&main, &["init", "-q"]);
    run(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
    main
}

#[test]
fn remotes_are_read_as_git_remote_get_url_prints_them() {
    let dir = tempfile::tempdir().unwrap();
    let main = repository(dir.path());
    run(&main, &["remote", "add", "origin", "gh:penso/app.git"]);
    run(
        &main,
        &["remote", "add", "scp", "git@gitlab.com:g/sub/app.git"],
    );
    run(
        &main,
        &["remote", "add", "web", "https://github.com/penso/web"],
    );
    run(&main, &["config", "url.git@github.com:.insteadOf", "gh:"]);
    let found = git::checkout(&main).unwrap().unwrap();
    assert_eq!(found.remotes.len(), 3);
    for (name, url) in &found.remotes {
        assert_eq!(url, &run(&main, &["remote", "get-url", name]), "{name}");
    }
    assert!(
        found
            .remotes
            .contains(&("origin".into(), "git@github.com:penso/app.git".into()))
    );
}

#[test]
fn a_linked_worktree_shares_its_main_checkout_and_common_directory() {
    let dir = tempfile::tempdir().unwrap();
    let main = repository(dir.path());
    let linked = dir.path().join("linked");
    run(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            &linked.to_string_lossy(),
            "-b",
            "side",
        ],
    );
    let sub = linked.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let from_main = git::checkout(&main).unwrap().unwrap();
    let from_linked = git::checkout(&sub).unwrap().unwrap();
    assert_eq!(from_linked.common, from_main.common);
    assert_eq!(
        from_linked.main.canonicalize().unwrap(),
        main.canonicalize().unwrap()
    );
    assert!(git::checkout(&dir.path().join("gone")).unwrap().is_none());
}

#[test]
fn agent_branches_and_base_commits_match_git() {
    let dir = tempfile::tempdir().unwrap();
    let main = repository(dir.path());
    for branch in ["agent/b", "agent/a", "other", "agent/c"] {
        run(&main, &["branch", branch]);
    }
    // A packed branch is listed with the loose ones.
    run(&main, &["pack-refs", "--all"]);
    run(&main, &["branch", "agent/d"]);
    assert_eq!(
        git::branches(&main, "agent/", 3).unwrap(),
        ["agent/a", "agent/b", "agent/c"]
    );
    assert_eq!(git::branches(&main, "agent/", 10).unwrap().len(), 4);

    run(&main, &["commit", "-q", "--allow-empty", "-m", "second"]);
    run(&main, &["tag", "-a", "v1", "-m", "annotated", "HEAD~1"]);
    let parse = |spec: &str| run(&main, &["rev-parse", &format!("{spec}^{{commit}}")]);
    for base in ["HEAD", "agent/a", "v1"] {
        assert_eq!(git::commit(&main, base).unwrap(), parse(base), "{base}");
    }
    let head = parse("HEAD");
    assert_eq!(git::commit(&main, &head).unwrap(), head);
    assert!(git::commit(&main, "no-such-branch").is_err());
}
