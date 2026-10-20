use super::*;
use crate::teleport::{Envelope, Process, ProcessInfo};
use serde::Deserialize;
use std::{fs::OpenOptions, io::Read, path::Path};

const RECORD_LIMIT: u64 = 16 * 1024;

#[derive(Deserialize)]
struct ProcessResult {
    process_info: LocatedProcesses,
}
#[derive(Deserialize)]
struct LocatedProcesses {
    pane_id: String,
    #[serde(flatten)]
    info: ProcessInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    pid: u32,
    kind: String,
    session_id: String,
    proc_start: String,
    #[serde(default)]
    pid_domain: Option<String>,
}

fn claude_executable(name: &str) -> bool {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    name == "claude" || name.eq_ignore_ascii_case("claude.exe")
}

fn process(info: &ProcessInfo) -> Result<&Process> {
    if info.foreground_process_group_id.is_none()
        || info.foreground_process_group_id == info.shell_pid
        || info.foreground_processes.len() > 256
    {
        return Err(Error::SessionProcesses);
    }
    let mut candidates = info.foreground_processes.iter().filter(|p| {
        p.pid > 0
            && p.name
                .iter()
                .chain(p.argv0.iter())
                .chain(p.argv.first())
                .any(|name| claude_executable(name))
    });
    let only = candidates.next().ok_or(Error::SessionProcesses)?;
    if candidates.next().is_some() {
        return Err(Error::SessionProcesses);
    }
    Ok(only)
}

fn normalize_start(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode(bytes: &[u8], runtime: &Runtime, domain: &str) -> Result<AgentSession> {
    let record: Record = serde_json::from_slice(bytes).map_err(chat::Error::Record)?;
    if record.pid != runtime.pid
        || record.kind != "interactive"
        || !chat::valid_session_id(&record.session_id)
        || record.pid_domain.as_deref().is_some_and(|d| d != domain)
        || record.proc_start.is_empty()
        || normalize_start(&record.proc_start) != runtime.started
    {
        return Err(chat::Error::Identity.into());
    }
    Ok(AgentSession {
        agent: "claude".into(),
        kind: SessionKind::Id,
        source: "claude:pid".into(),
        value: record.session_id,
    })
}

fn read_record(directory: &Path, runtime: &Runtime, domain: &str) -> Result<AgentSession> {
    let path = directory
        .join("sessions")
        .join(format!("{}.json", runtime.pid));
    let io_error = |source| chat::Error::Io {
        path: path.clone(),
        source,
    };
    // Validate the descriptor, including when the path is replaced during open.
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            (rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
        );
    }
    let file = options.open(&path).map_err(io_error)?;
    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() || metadata.len() > RECORD_LIMIT {
        return Err(chat::Error::Limit.into());
    }
    let mut bytes = Vec::new();
    file.take(RECORD_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > RECORD_LIMIT {
        return Err(chat::Error::Limit.into());
    }
    decode(&bytes, runtime, domain)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(super) fn resolve(
    host: &Host,
    pane: &str,
    default_directory: &Path,
    stop: &AtomicBool,
) -> Result<Resolved> {
    let output = host
        .herdr_exec(
            &["pane", "process-info", "--pane", pane],
            std::time::Duration::from_secs(5),
            stop,
        )
        .map_err(Error::Query)?;
    let result: Envelope<ProcessResult> = serde_json::from_slice(&output).map_err(Error::Decode)?;
    if result.result.process_info.pane_id != pane {
        return Err(Error::TargetChanged);
    }
    let pid = process(&result.result.process_info.info)?.pid;
    let (runtime, directory) = inspect_process(pid, default_directory)?;
    let domain = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let session = read_record(&directory, &runtime, domain)?;
    Ok(Resolved {
        session,
        directory,
        runtime: Some(runtime),
    })
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn inspect_process(pid: u32, default_directory: &Path) -> Result<(Runtime, PathBuf)> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let pid_key = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid_key]),
        true,
        ProcessRefreshKind::nothing().with_environ(UpdateKind::Always),
    );
    let process = system.process(pid_key).ok_or(chat::Error::Identity)?;
    let configured = process
        .environ()
        .iter()
        .find(|value| value.as_encoded_bytes().starts_with(b"CLAUDE_CONFIG_DIR="));
    let directory = match configured {
        Some(value) => {
            let value = value
                .to_str()
                .ok_or(Error::Config)?
                .strip_prefix("CLAUDE_CONFIG_DIR=")
                .ok_or(Error::Config)?;
            if value.is_empty() {
                default_directory.to_owned()
            } else {
                PathBuf::from(value)
            }
        }
        None => default_directory.to_owned(),
    };
    if !directory.is_absolute() {
        return Err(chat::Error::Identity.into());
    }
    #[cfg(target_os = "macos")]
    let started = mac_start(process.start_time())?;
    #[cfg(target_os = "linux")]
    let started = linux_start(pid)?;
    Ok((Runtime { pid, started }, directory))
}

#[cfg(any(test, target_os = "macos"))]
fn mac_start(seconds: u64) -> Result<String> {
    let at = i64::try_from(seconds)
        .ok()
        .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
        .filter(|_| seconds > 0)
        .ok_or(chat::Error::Identity)?;
    Ok(normalize_start(
        &at.format("%a %b %e %H:%M:%S %Y").to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn linux_start(pid: u32) -> Result<String> {
    let path = PathBuf::from(format!("/proc/{pid}/stat"));
    let file = std::fs::File::open(&path).map_err(|source| chat::Error::Io {
        path: path.clone(),
        source,
    })?;
    let mut stat = String::new();
    file.take(64 * 1024)
        .read_to_string(&mut stat)
        .map_err(|source| chat::Error::Io { path, source })?;
    linux_start_in(&stat).ok_or_else(|| chat::Error::Identity.into())
}

#[cfg(any(test, target_os = "linux"))]
fn linux_start_in(stat: &str) -> Option<String> {
    let value = stat.rsplit_once(") ")?.1.split_whitespace().nth(19)?;
    (!value.is_empty() && value.bytes().all(|c| c.is_ascii_digit())).then(|| value.into())
}

#[cfg(test)]
mod tests;
