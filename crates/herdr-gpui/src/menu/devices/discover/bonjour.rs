//! SSH servers that announce `_ssh._tcp` over multicast DNS: a Mac with Remote
//! Login on, or a Linux host whose Avahi publishes its SSH service. Hosts that
//! do not announce themselves are not found; nothing is scanned.

use super::{Candidate, Source, valid_host};
use crate::{Error, Result};
use mdns_sd::{RecvTimeoutError, ServiceDaemon};
use std::{
    collections::HashSet,
    io,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    time::{Duration, Instant},
};

const SERVICE: &str = "_ssh._tcp.local.";
/// How often the query is sent again while nothing has answered.
const REQUERY: Duration = Duration::from_secs(1);

/// Listen for `duration`, reporting each resolved announcement that is not
/// this machine until `found` returns `false`. Blocks: call it from the
/// background executor.
///
/// The first time a process reaches the local network, macOS checks its
/// Local Network permission and drops replies until the check is done, so
/// the first queries of a fresh launch can go unanswered. The library backs
/// off to queries 1, 3 and 7 seconds in; until something answers, the browse
/// restarts every second instead, which sends a new query each time.
pub(super) fn browse(duration: Duration, mut found: impl FnMut(Candidate) -> bool) -> Result<()> {
    let local: HashSet<IpAddr> = if_addrs::get_if_addrs()
        .map_err(Error::LocalAddresses)?
        .into_iter()
        .map(|interface| interface.ip())
        .collect();
    if cfg!(target_os = "macos") && local.iter().any(|ip| ip.is_ipv4() && !ip.is_loopback()) {
        local_network_allowed()?;
    }
    let daemon = ServiceDaemon::new()?;
    let mut events = daemon.browse(SERVICE)?;
    let deadline = Instant::now() + duration;
    let mut answered = false;
    loop {
        let wake = if answered {
            deadline
        } else {
            deadline.min(Instant::now() + REQUERY)
        };
        let event = match events.recv_deadline(wake) {
            Ok(event) => event,
            Err(RecvTimeoutError::Timeout) if !answered && Instant::now() < deadline => {
                // Replaces the listener, so the old receiver gets nothing more.
                let Ok(again) = daemon.browse(SERVICE) else {
                    break;
                };
                events = again;
                continue;
            }
            // Timed out, or the daemon stopped on its own.
            Err(_) => break,
        };
        let mdns_sd::ServiceEvent::ServiceResolved(service) = event else {
            continue;
        };
        answered = true;
        let addresses: Vec<IpAddr> = service
            .get_addresses()
            .iter()
            .map(|address| address.to_ip_addr())
            .collect();
        let Some(candidate) = candidate(
            service.get_fullname(),
            service.get_hostname(),
            service.get_port(),
            &addresses,
            &local,
        ) else {
            continue;
        };
        if !found(candidate) {
            break;
        }
    }
    // What was found stands even if the daemon's thread fails to stop
    // cleanly; it exits with this process either way.
    let _ = daemon.shutdown();
    Ok(())
}

/// macOS fails a multicast send with "no route to host" while Herdr lacks
/// Local Network permission. The mDNS daemon only logs that, so the browse
/// would find nothing and look like an empty network; one send first tells
/// the two apart. The packet is a DNS header asking nothing, which responders
/// ignore. Any other failure is left to the browse.
fn local_network_allowed() -> Result<()> {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else {
        return Ok(());
    };
    multicast_sent(socket.send_to(&[0; 12], (Ipv4Addr::new(224, 0, 0, 251), 5353)))
}

pub(super) fn multicast_sent(result: io::Result<usize>) -> Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::HostUnreachable => {
            Err(Error::LocalNetworkDenied)
        }
        _ => Ok(()),
    }
}

/// The suggestion for one announcement, or `None` for this machine or a host
/// name unsafe to use as an SSH target.
pub(super) fn candidate(
    fullname: &str,
    hostname: &str,
    port: u16,
    addresses: &[IpAddr],
    local: &HashSet<IpAddr>,
) -> Option<Candidate> {
    if addresses.iter().any(|address| local.contains(address)) {
        return None;
    }
    let host = hostname.trim_end_matches('.');
    if !valid_host(host) || port == 0 {
        return None;
    }
    let instance = fullname
        .strip_suffix(SERVICE)
        .unwrap_or(fullname)
        .trim_end_matches('.');
    let name = if instance.is_empty() { host } else { instance };
    let target = if port == 22 {
        host.to_owned()
    } else {
        format!("ssh://{host}:{port}")
    };
    let addresses: Vec<String> = addresses.iter().map(IpAddr::to_string).collect();
    let hosts: Vec<&str> = std::iter::once(host)
        .chain(addresses.iter().map(String::as_str))
        .collect();
    Some(Candidate::new(Source::Bonjour, name, target, &hosts))
}
