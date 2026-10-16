use super::*;

#[test]
fn workspace_choices_offer_new_workspace_then_other_workspaces() {
    let snapshot = snapshot();
    assert_eq!(
        workspace_choices(&snapshot, "w1"),
        [
            Choice {
                destination: Destination::NewWorkspace,
                workspace: None,
                place: "New Workspace".into(),
            },
            Choice {
                destination: Destination::NewTab {
                    workspace_id: "w2".into()
                },
                workspace: None,
                place: "other".into(),
            },
        ]
    );
}

#[test]
fn tab_request_validates_every_pane_and_destination() {
    assert!(matches!(
        TabRequest::new(Vec::new(), Destination::NewWorkspace, None),
        Err(Error::InvalidId(rejected)) if rejected.is_empty()
    ));
    for id in ["", "--new-workspace", "w1\np2"] {
        assert!(matches!(
            TabRequest::new(vec!["w1:p1".into(), id.into()], Destination::NewWorkspace, None),
            Err(Error::InvalidId(rejected)) if rejected == id
        ));
    }
    assert!(matches!(
        TabRequest::new(
            vec!["w1:p1".into()],
            Destination::NewWorkspace,
            Some("bad\nname".into()),
        ),
        Err(Error::InvalidId(rejected)) if rejected == "bad\nname"
    ));
    assert!(
        TabRequest::new(
            vec!["w1:p1".into(), "w1:p2".into()],
            Destination::NewTab {
                workspace_id: "w2".into()
            },
            Some("my-tab".into()),
        )
        .is_ok()
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn tab_request_moves_all_panes_into_the_created_tab() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("herdr");
    std::fs::write(
        &fake,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/calls"
if [ "$3" = "tab" ] && [ "$4" = "rename" ]; then
  printf '%s\n' '{"result":{"type":"tab_rename"}}'
  exit 0
fi
case "$5" in
  w1:p1)
    printf '%s\n' '{"result":{"type":"pane_move","move_result":{"changed":true,"created_tab":{"tab_id":"w2:t9"},"pane":{"tab_id":"w2:t9"}}}}'
    ;;
  w1:p2|w1:p3)
    printf '%s\n' '{"result":{"type":"pane_move","move_result":{"changed":true,"pane":{"tab_id":"w2:t9"}}}}'
    ;;
  *)
    printf '%s\n' '{"id":"cli:pane:move","error":{"code":"pane_not_found","message":"pane not found"}}' >&2
    exit 1
    ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut host = host_for(&ConnectTarget::Session {
        name: "work".into(),
        development: false,
    })
    .unwrap();
    host.env = vec![
        ("HOME".into(), home.path().to_string_lossy().into_owned()),
        ("PATH".into(), "/usr/bin:/bin".into()),
    ];
    let cancelled = AtomicBool::new(false);
    let request = TabRequest::new(
        vec!["w1:p1".into(), "w1:p2".into(), "w1:p3".into()],
        Destination::NewTab {
            workspace_id: "w2".into(),
        },
        Some("tab-name".into()),
    )
    .unwrap();
    request.run(&host, &cancelled).unwrap();
    assert_eq!(
        std::fs::read_to_string(home.path().join("calls")).unwrap(),
        "--session work pane move w1:p1 --new-tab --workspace w2 --focus\n\
         --session work tab rename w2:t9 tab-name\n\
         --session work pane move w1:p2 --tab w2:t9 --split right --focus\n\
         --session work pane move w1:p3 --tab w2:t9 --split right --focus\n"
    );
}
