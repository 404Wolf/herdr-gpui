//! Each host's token, read by running its `token_command`. The command's
//! output is the secret; it is never logged, and an answer is kept in memory
//! for a few minutes so a refresh of many checkouts runs it once.

use super::{Error, Result, registry::Host};
use secrecy::SecretString;
use std::{
    io::Read,
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const TIMEOUT: Duration = Duration::from_secs(10);
/// How long a read token is reused before the command runs again, so a
/// rotated token is picked up without restarting the app.
const TTL: Duration = Duration::from_secs(5 * 60);
/// A token is at most a few hundred bytes; the rest is room for a newline.
const OUTPUT_LIMIT: usize = 16 * 1024;

type Cached = (String, Vec<String>, Instant, Option<Arc<SecretString>>);

static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());

/// The token `host` is read with, or `None` for anonymous reads: a host with
/// no command, or one served over plain HTTP, where a token would be exposed.
/// Blocks on a subprocess: call it from a worker.
pub fn token(host: &Host) -> Result<Option<Arc<SecretString>>> {
    if host.token_command.is_empty() || !host.secure {
        return Ok(None);
    }
    let now = Instant::now();
    if let Ok(cache) = CACHE.lock()
        && let Some((.., token)) = cache.iter().find(|(name, command, read, _)| {
            *name == host.host && *command == host.token_command && now.duration_since(*read) < TTL
        })
    {
        return Ok(token.clone());
    }
    let token = run(host)?.map(Arc::new);
    if let Ok(mut cache) = CACHE.lock() {
        cache.retain(|(name, ..)| *name != host.host);
        cache.push((
            host.host.clone(),
            host.token_command.clone(),
            now,
            token.clone(),
        ));
    }
    Ok(token)
}

fn run(host: &Host) -> Result<Option<SecretString>> {
    let failed = |reason: &str| Error::TokenCommand {
        host: host.host.clone(),
        reason: reason.to_owned(),
    };
    let (program, args) = host
        .token_command
        .split_first()
        .ok_or_else(|| failed("it is empty"))?;
    let mut command = herdr_client::new_command(program);
    // An app opened from a desktop launcher has a minimal PATH.
    herdr_client::login_env::apply(&mut command);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| failed(&format!("it could not start ({})", error.kind())))?;
    let mut stdout = child.stdout.take().ok_or_else(|| failed("no output"))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("herdr-gitea-token".into())
        .spawn(move || {
            let mut bytes = Zeroizing::new(Vec::new());
            let read = (&mut stdout)
                .take(OUTPUT_LIMIT as u64 + 1)
                .read_to_end(&mut bytes);
            let _ = sender.send(read.map(|_| bytes));
        })
        .map_err(|_| failed("its output could not be read"))?;
    let deadline = Instant::now() + TIMEOUT;
    let output = loop {
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => break result.map_err(|_| failed("its output could not be read"))?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failed("it did not finish in time"));
            }
        }
    };
    let status = child
        .wait()
        .map_err(|_| failed("its exit status could not be read"))?;
    if !status.success() {
        return Err(failed("it exited unsuccessfully"));
    }
    if output.len() > OUTPUT_LIMIT {
        return Err(failed("it printed too much"));
    }
    parse(&output, &host.host)
}

/// The command prints the token alone, perhaps with a trailing newline.
pub(crate) fn parse(output: &[u8], host: &str) -> Result<Option<SecretString>> {
    let text = std::str::from_utf8(output).map_err(|_| Error::Token(host.to_owned()))?;
    let token = text.trim();
    if token.is_empty() {
        return Ok(None);
    }
    if token.len() > 1024 || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(Error::Token(host.to_owned()));
    }
    Ok(Some(token.into()))
}
