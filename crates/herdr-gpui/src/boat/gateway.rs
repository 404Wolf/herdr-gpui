//! A pinned OpenSSH command for one Boat sandbox. The app owns one persistent
//! private key; Boat authorizes its public half on each running machine and
//! returns that machine's host key over the authenticated API connection.

use super::{Error, Result, api::SshAccess};
use std::{
    fs,
    io::{ErrorKind, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
};

const ALIAS: &str = "herdr-boat-sandbox";
const PRIVATE_KEY: &str = "boat_ed25519";
static KEY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

// OpenSSH expands percent tokens and parses whitespace even in an argv value.
fn quoted_path(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or(Error::Gateway)?;
    if !path.is_absolute() || text.contains(['"', '\n', '\r', '\0']) {
        return Err(Error::Gateway);
    }
    Ok(format!("\"{}\"", text.replace('%', "%%")))
}

fn private_key() -> Result<PathBuf> {
    let _guard = KEY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory =
        crate::preferences::state_dir().ok_or(Error::Cloud(crate::cloud::Error::StateDirectory))?;
    fs::create_dir_all(&directory).map_err(Error::SshKey)?;
    let destination = directory.join(PRIVATE_KEY);
    if destination.is_file() {
        return Ok(destination);
    }
    if destination.exists() {
        return Err(Error::SshKey(std::io::Error::other(
            "Boat SSH key path is not a file",
        )));
    }
    let temporary = tempfile::tempdir_in(&directory).map_err(Error::SshKey)?;
    let generated = temporary.path().join(PRIVATE_KEY);
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&generated)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(Error::SshMissing)?;
    if !status.success() {
        return Err(Error::SshKeygen);
    }
    // Publish without replacing a key another app process may just have won.
    match fs::hard_link(&generated, &destination) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists && destination.is_file() => {}
        Err(error) => return Err(Error::SshKey(error)),
    }
    Ok(destination)
}

fn public_key(private: &Path) -> Result<String> {
    let output = Command::new("ssh-keygen")
        .args(["-y", "-f"])
        .arg(private)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(Error::SshMissing)?;
    if !output.status.success() {
        return Err(Error::SshKeygen);
    }
    let key =
        String::from_utf8(output.stdout).map_err(|_| Error::Encoding("Boat SSH public key"))?;
    let key = key.trim();
    if key.len() > 4096
        || !key.starts_with("ssh-ed25519 ")
        || key.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(Error::Gateway);
    }
    Ok(key.to_owned())
}

fn user(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn host_key(value: &str) -> bool {
    let Some((kind, data)) = value.split_once(' ') else {
        return false;
    };
    matches!(kind, "ssh-ed25519" | "ecdsa-sha2-nistp256" | "ssh-rsa")
        && !data.is_empty()
        && data.len() <= 4096
        && data
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

fn endpoint(access: &SshAccess) -> Result<(String, u16)> {
    if let Some(endpoint) = &access.ssh_endpoint {
        let (host, port) = endpoint.rsplit_once(':').ok_or(Error::Gateway)?;
        let host: IpAddr = host.parse().map_err(|_| Error::Gateway)?;
        let port = port.parse().map_err(|_| Error::Gateway)?;
        if port == 0 {
            return Err(Error::Gateway);
        }
        return Ok((host.to_string(), port));
    }
    let host: IpAddr = access
        .machine_ip
        .as_deref()
        .ok_or(Error::Gateway)?
        .parse()
        .map_err(|_| Error::Gateway)?;
    Ok((host.to_string(), 22))
}

pub(crate) struct Gateway {
    pub(crate) command: Command,
    _known_hosts: tempfile::NamedTempFile,
}

impl Gateway {
    /// Keep the pinned host-key file alive until the operation finishes.
    pub(crate) fn with_command<T>(self, run: impl FnOnce(Command) -> T) -> T {
        run(self.command)
    }

    pub(crate) fn new(access: &SshAccess, private: &Path) -> Result<Self> {
        if !user(&access.ssh_user) || !host_key(&access.host_key) {
            return Err(Error::Gateway);
        }
        let (host, port) = endpoint(access)?;
        let mut known_hosts = tempfile::NamedTempFile::new().map_err(Error::GatewayFile)?;
        writeln!(known_hosts, "{ALIAS} {}", access.host_key)
            .and_then(|()| known_hosts.flush())
            .map_err(Error::GatewayFile)?;
        let known_hosts_path = quoted_path(known_hosts.path())?;
        let mut command = Command::new("ssh");
        command
            .args(["-F", "/dev/null", "-T"])
            .arg("-o")
            .arg(format!("IdentityFile={}", quoted_path(private)?))
            .args(["-o", "IdentitiesOnly=yes"])
            .args(["-o", "BatchMode=yes"])
            .args(["-o", "NumberOfPasswordPrompts=0"])
            .args(["-o", "StrictHostKeyChecking=yes"])
            .arg("-o")
            .arg(format!("UserKnownHostsFile={known_hosts_path}"))
            .args(["-o", "GlobalKnownHostsFile=/dev/null"])
            .args(["-o", &format!("HostKeyAlias={ALIAS}")])
            .args(["-o", "ConnectTimeout=15"])
            .args(["-o", "ConnectionAttempts=1"])
            .args(["-o", "ServerAliveInterval=15"])
            .args(["-o", "ServerAliveCountMax=4"])
            .args(["-o", "ForwardAgent=no"])
            .args(["-o", "ForwardX11=no"])
            .args(["-o", "ClearAllForwardings=yes"])
            .args(["-o", "ControlMaster=no"])
            .args(["-o", "LogLevel=ERROR"])
            .args(["-p", &port.to_string(), "-l", &access.ssh_user, "--"])
            .arg(host);
        Ok(Self {
            command,
            _known_hosts: known_hosts,
        })
    }
}

pub(crate) fn prepare(request: &super::http::Request, id: &str) -> Result<Gateway> {
    let private = private_key()?;
    let public = public_key(&private)?;
    Gateway::new(&request.ssh_access(id, &public)?, &private)
}

pub(crate) fn check_ssh() -> Result<()> {
    for program in ["ssh", "ssh-keygen"] {
        Command::new(program)
            .arg("-V")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(drop)
            .map_err(Error::SshMissing)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
