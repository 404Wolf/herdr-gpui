#![allow(clippy::unwrap_used)]
use super::*;
use serde_json::json;

const ID: &str = "00000000-0000-4000-8000-000000000001";

fn record(pid: u32, start: &str) -> serde_json::Value {
    json!({ "pid":pid, "kind":"interactive", "sessionId":ID, "procStart":start, "pidDomain":"darwin" })
}

#[test]
fn selects_claude_by_argv_even_when_the_executable_name_is_its_version() {
    let payload = json!({"result":{"process_info":{
        "pane_id":"w1:p3", "shell_pid":10, "foreground_process_group_id":20,
        "foreground_processes":[
            {"pid":21,"name":"node","argv":["node","mcp-server"]},
            {"pid":20,"name":"2.1.304","argv0":"claude","argv":["claude"]}
        ]
    }}});
    let parsed: Envelope<ProcessResult> = serde_json::from_value(payload).unwrap();
    assert_eq!(parsed.result.process_info.pane_id, "w1:p3");
    let mut info = parsed.result.process_info.info;
    assert_eq!(process(&info).unwrap().pid, 20);
    info.foreground_processes.push(Process {
        pid: 30,
        argv0: Some("/usr/local/bin/claude".into()),
        ..Default::default()
    });
    assert!(matches!(process(&info), Err(Error::SessionProcesses)));
    info.foreground_processes.pop();
    info.foreground_process_group_id = info.shell_pid;
    assert!(matches!(process(&info), Err(Error::SessionProcesses)));
}

#[test]
fn verifies_process_birth_and_rejects_reused_pids_or_other_session_kinds() {
    let runtime = Runtime {
        pid: 42,
        started: "Thu Jan 1 00:00:01 1970".into(),
    };
    let valid = record(42, "Thu Jan  1 00:00:01 1970");
    let bytes = serde_json::to_vec(&valid).unwrap();
    assert_eq!(decode(&bytes, &runtime, "darwin").unwrap().value, ID);
    for (field, value) in [
        ("pid", json!(43)),
        ("procStart", json!("Thu Jan 1 00:00:02 1970")),
        ("kind", json!("background")),
        ("sessionId", json!("../other")),
        ("pidDomain", json!("linux")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        assert!(matches!(
            decode(&serde_json::to_vec(&invalid).unwrap(), &runtime, "darwin"),
            Err(Error::Transcript(chat::Error::Identity))
        ));
    }
    let mut old = valid;
    old.as_object_mut().unwrap().remove("procStart");
    assert!(matches!(
        decode(&serde_json::to_vec(&old).unwrap(), &runtime, "darwin"),
        Err(Error::Transcript(chat::Error::Record(_)))
    ));
}

#[test]
fn pid_records_are_read_afresh_when_clear_or_resume_changes_sessions() {
    let directory = tempfile::tempdir().unwrap();
    let sessions = directory.path().join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let path = sessions.join("42.json");
    let runtime = Runtime {
        pid: 42,
        started: "birth".into(),
    };
    let mut data = record(42, "birth");
    std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(
        read_record(directory.path(), &runtime, "darwin")
            .unwrap()
            .value,
        ID
    );
    let next = "00000000-0000-4000-8000-000000000002";
    data["sessionId"] = json!(next);
    std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(
        read_record(directory.path(), &runtime, "darwin")
            .unwrap()
            .value,
        next
    );
    std::fs::write(&path, b"{\"pid\":").unwrap();
    assert!(read_record(directory.path(), &runtime, "darwin").is_err());
    std::fs::write(&path, vec![b' '; RECORD_LIMIT as usize + 1]).unwrap();
    assert!(matches!(
        read_record(directory.path(), &runtime, "darwin"),
        Err(Error::Transcript(chat::Error::Limit))
    ));
}

#[test]
fn process_start_formats_match_claudes_native_records() {
    assert_eq!(mac_start(1).unwrap(), "Thu Jan 1 00:00:01 1970");
    assert!(mac_start(0).is_err());
    // stat's comm can contain spaces and closing parentheses; field 22 is
    // starttime, index 19 after the final closing parenthesis.
    let stat = format!(
        "42 (claude (worker)) S {} 123456 0 0",
        vec!["0"; 18].join(" ")
    );
    assert_eq!(linux_start_in(&stat).as_deref(), Some("123456"));
    assert!(linux_start_in("42 (claude) S 0").is_none());
}

#[cfg(unix)]
#[test]
fn symlinks_and_nonregular_pid_records_are_refused() {
    let directory = tempfile::tempdir().unwrap();
    let sessions = directory.path().join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    let other = directory.path().join("other.json");
    std::fs::write(&other, serde_json::to_vec(&record(42, "birth")).unwrap()).unwrap();
    let path = sessions.join("42.json");
    std::os::unix::fs::symlink(&other, &path).unwrap();
    let runtime = Runtime {
        pid: 42,
        started: "birth".into(),
    };
    assert!(read_record(directory.path(), &runtime, "darwin").is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(read_record(directory.path(), &runtime, "darwin").is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn native_macos_birth_matches_ps_in_utc_and_c_locale() {
    let pid = std::process::id();
    let directory = tempfile::tempdir().unwrap();
    let (runtime, _) = inspect_process(pid, directory.path()).unwrap();
    let mut ps = herdr_client::new_command("/bin/ps");
    ps.env_clear().env("LC_ALL", "C").env("TZ", "UTC").args([
        "-o",
        "lstart=",
        "-p",
        &pid.to_string(),
    ]);
    let output = Host::capture_command(
        ps,
        std::time::Duration::from_secs(5),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        runtime.started,
        normalize_start(&String::from_utf8(output).unwrap())
    );
}
