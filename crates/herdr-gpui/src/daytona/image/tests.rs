use super::*;

fn snapshot(state: &str, reason: Option<&str>) -> Snapshot {
    Snapshot {
        state: state.into(),
        error_reason: reason.map(Into::into),
    }
}

#[test]
fn snapshot_states_map_onto_a_build() {
    assert_eq!(snapshot("active", None).readiness(), Readiness::Ready);
    assert_eq!(
        snapshot("build_failed", Some("RUN exited 1")).readiness(),
        Readiness::Failed("RUN exited 1".into())
    );
    assert_eq!(
        snapshot("error", Some("")).readiness(),
        Readiness::Failed("error".into())
    );
    assert_eq!(
        snapshot("pending_build", None).readiness(),
        Readiness::Pending("pending build".into())
    );
}

#[test]
fn snapshot_names_are_safe_in_paths() {
    let project = Project {
        id: "0123456789abcdef".into(),
        label: "app".into(),
    };
    let name = name(&project, &"b".repeat(64));
    assert_eq!(name, "herdr-app-01234567-bbbbbbbbbbbb");
    assert!(segment(&name).is_ok());
    assert!(segment("../x").is_err());
    assert!(segment("a/b").is_err());
    assert!(segment("").is_err());
}
