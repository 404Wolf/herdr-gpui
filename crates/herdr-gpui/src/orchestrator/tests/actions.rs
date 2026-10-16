use super::*;
use crate::orchestrator::actions::{Action, DispatchRequest, Site, perform};
use crate::teleport::{AgentKind, Host};
use herdr_client::ConnectTarget;
use std::{os::unix::fs::PermissionsExt, path::Path, process::Command, sync::atomic::AtomicBool};

mod agent_name;
mod parent;
mod remove;
mod startup;

/// A home whose `herdr` logs each call and answers as Herdr would. A call
/// whose arguments contain `$FAIL` exits 1. Its snapshot is `$HOME/snapshot`
/// when written, else one where `w1` is the repository's parent workspace
/// and `w9` the linked worktree a dispatch creates.
fn fake_home(dir: &Path, fail: &str) -> Host {
    let bin = dir.join("home/.local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let snapshot = snapshot(&[("w1", false), ("w9", true)]);
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/calls"
if [ -n "{fail}" ]; then case "$*" in *"{fail}"*) exit 1 ;; esac; fi
case "$1 $2" in
  "api snapshot")
    if [ -f "$HOME/snapshot" ]; then cat "$HOME/snapshot"; else printf '%s' '{snapshot}'; fi ;;
  "workspace create") printf '%s' '{{"result":{{"workspace":{{"workspace_id":"w0"}}}}}}' ;;
  "worktree create") printf '%s' '{{"result":{{"workspace":{{"workspace_id":"w9"}},"tab":{{"tab_id":"t9"}},"root_pane":{{"pane_id":"p9","terminal_id":"term9"}},"worktree":{{"path":"/tmp/wt/377-icons"}}}}}}' ;;
  "pane get")
    if [ -f "$HOME/pane" ]; then cat "$HOME/pane"; else printf '%s' '{{"result":{{"pane":{{"pane_id":"p9","terminal_id":"term9"}}}}}}'; fi ;;
  "agent start")
    if [ -f "$HOME/busy" ]; then
      read -r busy < "$HOME/busy"
      if [ "$busy" -gt 0 ]; then
        printf '%s\n' "$((busy - 1))" > "$HOME/busy"
        printf '%s' '{{"id":"cli:agent:start","error":{{"code":"agent_pane_busy","message":"not an available shell"}}}}' >&2
        exit 1
      fi
    fi ;;
esac
"#
    );
    std::fs::write(bin.join("herdr"), script).unwrap();
    std::fs::set_permissions(bin.join("herdr"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut host = Host::new(&ConnectTarget::Local).unwrap();
    host.env = vec![(
        "HOME".into(),
        dir.join("home").to_string_lossy().into_owned(),
    )];
    host
}

/// `herdr api snapshot` output with each `(workspace, linked)` a checkout of
/// one repository.
fn snapshot(workspaces: &[(&str, bool)]) -> String {
    let workspaces: Vec<_> = workspaces
        .iter()
        .map(|(id, linked)| {
            serde_json::json!({
                "workspace_id": id,
                "worktree": {
                    "repo_key": "/r/.git",
                    "repo_name": "r",
                    "repo_root": "/r",
                    "checkout_path": format!("/r/{id}"),
                    "is_linked_worktree": linked,
                },
            })
        })
        .collect();
    serde_json::json!({
        "id": "cli:api:snapshot",
        "result": {"snapshot": {"workspaces": workspaces, "tabs": [], "panes": []}},
    })
    .to_string()
}

fn repository(dir: &Path) -> String {
    let root = dir.join("app");
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "base",
    ]);
    root.to_string_lossy().into_owned()
}

fn site(dir: &Path, fail: &str) -> Site {
    Site {
        host: fake_home(dir, fail),
        target: ConnectTarget::Local,
        main_root: repository(dir),
        database: dir.join("db/id/state.sqlite3"),
    }
}

fn request() -> DispatchRequest {
    DispatchRequest {
        item_key: "github:github.com:penso/herdr-gpui:377".into(),
        kind: AgentKind::Claude,
        model: None,
        prompt: "Fix the icons; don't break 'quotes'.".into(),
        branch: "377-icons".into(),
        workspace_id: "w1".into(),
        base: None,
        elsewhere: None,
    }
}

fn calls(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("home/calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn dispatch_creates_the_worktree_starts_the_agent_and_records_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    let done = perform(
        &site,
        &Action::Dispatch(Box::new(request())),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(done, "Agent started");
    let calls = calls(dir.path());
    assert_eq!(calls[0], "api snapshot");
    assert!(calls[1].starts_with("worktree create --workspace w1 --branch 377-icons --base "));
    assert!(calls[1].ends_with(" --no-focus"));
    let name = calls[2].split(' ').nth(2).unwrap();
    assert!(
        name.starts_with("herdr-gpui-") && name.len() == 32,
        "{name}"
    );
    assert_eq!(
        calls[2],
        format!("agent start {name} --kind claude --pane p9 --timeout 90000")
    );
    assert_eq!(
        calls[3],
        "agent prompt p9 Fix the icons; don't break 'quotes'."
    );

    let store = Store::open(&site.database).unwrap();
    let run = store.runs().unwrap().remove(0);
    assert_eq!(
        (run.owner, run.state, run.agent.as_str()),
        (Owner::HerdrGpui, RunState::Running, "claude")
    );
    let workspace = run.workspace.unwrap();
    assert_eq!(
        (workspace.id.as_str(), workspace.branch.as_str()),
        ("w9", "377-icons")
    );
    let session = store.sessions().unwrap().remove(0);
    assert_eq!(
        (session.pane_id.as_str(), session.agent_name.as_str()),
        ("p9", name)
    );
    assert_eq!(session.host, None);
}

#[test]
fn a_failed_start_removes_the_worktree_and_records_why() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "agent start");
    let error = perform(
        &site,
        &Action::Dispatch(Box::new(request())),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        Error::Script {
            operation: "Starting the agent",
            ..
        }
    ));
    assert_eq!(
        calls(dir.path()).last().unwrap(),
        "worktree remove --workspace w9 --force"
    );
    let run = Store::open(&site.database)
        .unwrap()
        .runs()
        .unwrap()
        .remove(0);
    assert_eq!(run.state, RunState::Failed);
    assert!(run.message.unwrap().contains("Starting the agent"));
}

#[test]
fn stop_and_remove_act_on_own_runs_only() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    let cancelled = AtomicBool::new(false);
    perform(&site, &Action::Dispatch(Box::new(request())), &cancelled).unwrap();
    let id = Store::open(&site.database).unwrap().runs().unwrap()[0]
        .id
        .clone();

    perform(&site, &Action::Stop { run: id.clone() }, &cancelled).unwrap();
    let calls_now = calls(dir.path());
    assert_eq!(
        &calls_now[calls_now.len() - 2..],
        ["agent send-keys p9 esc", "agent send-keys p9 ctrl+c"]
    );
    let run = Store::open(&site.database)
        .unwrap()
        .runs()
        .unwrap()
        .remove(0);
    assert_eq!(run.state, RunState::Cancelled);

    perform(&site, &Action::Remove { run: id.clone() }, &cancelled).unwrap();
    assert_eq!(
        calls(dir.path()).last().unwrap(),
        "worktree remove --workspace w9 --force"
    );
    assert!(
        Store::open(&site.database)
            .unwrap()
            .runs()
            .unwrap()
            .is_empty()
    );

    // agent-launcher's run is shown, never acted on.
    rusqlite::Connection::open(&site.database)
        .unwrap()
        .execute_batch(
            "INSERT INTO runs (id, issue_key, agent, state_json, started_at, updated_at) VALUES \
             ('theirs', 'k', 'opencode', '\"running\"', '2026-10-08T00:00:00+00:00', '2026-10-08T00:00:00+00:00')",
        )
        .unwrap();
    let before = calls(dir.path()).len();
    for action in [
        Action::Stop {
            run: "theirs".into(),
        },
        Action::Remove {
            run: "theirs".into(),
        },
        Action::Send {
            run: "theirs".into(),
            text: "hi".into(),
        },
    ] {
        assert!(matches!(
            perform(&site, &action, &cancelled),
            Err(Error::NotOwner(_))
        ));
    }
    assert_eq!(
        calls(dir.path()).len(),
        before,
        "nothing ran for a foreign run"
    );
}

#[test]
fn a_chosen_model_reaches_the_agent_and_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    let mut with_model = request();
    with_model.model = Some("claude-opus-5-5".into());
    perform(
        &site,
        &Action::Dispatch(Box::new(with_model)),
        &AtomicBool::new(false),
    )
    .unwrap();
    let start = calls(dir.path())[2].clone();
    assert!(
        start.ends_with(" --timeout 90000 -- --model claude-opus-5-5"),
        "{start}"
    );
    let run = Store::open(&site.database)
        .unwrap()
        .runs()
        .unwrap()
        .remove(0);
    assert_eq!(run.model.as_deref(), Some("claude-opus-5-5"));
}

#[test]
fn only_plain_model_names_are_passed_on() {
    use crate::orchestrator::actions::valid_model;
    for good in [
        "gpt-5.5",
        "anthropic/claude-sonnet-5-5",
        "qwen3:32b",
        "model@latest",
    ] {
        assert!(valid_model(good), "{good}");
    }
    for bad in [
        "",
        "--dangerously-skip-permissions",
        "a b",
        "x;rm",
        &"m".repeat(129),
    ] {
        assert!(!valid_model(bad), "{bad}");
    }
}

#[test]
fn a_run_dispatched_elsewhere_is_reached_on_its_own_host() {
    use crate::orchestrator::actions::target_of;
    let mut on = session("r");
    on.host = Some("devbox".into());
    on.session = Some("work".into());
    assert_eq!(
        target_of(&on),
        ConnectTarget::Ssh {
            target: "devbox".into(),
            session: "work".into()
        }
    );
    on.session = None;
    assert_eq!(
        target_of(&on),
        ConnectTarget::Ssh {
            target: "devbox".into(),
            session: "default".into()
        }
    );
    on.host = None;
    assert_eq!(target_of(&on), ConnectTarget::Local);
    on.session = Some("dev".into());
    assert_eq!(
        target_of(&on),
        ConnectTarget::Session {
            name: "dev".into(),
            development: false
        }
    );
}
