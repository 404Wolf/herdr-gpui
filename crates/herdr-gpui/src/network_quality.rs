//! Latency and loss from this machine to the internet, for the status bar.
//! A worker thread opens one TCP connection every fifteen seconds and times
//! the handshake. That is a few hundred bytes a probe and about a megabyte a
//! day, so a metered link such as a phone hotspot can afford it, unlike a
//! throughput test. The UI thread only starts and stops the worker and takes
//! its probes on later ticks.

mod probe;
mod render;

#[cfg(test)]
mod tests;

use crate::system_load::Stop;
use probe::Outcome;
use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

const INTERVAL: Duration = Duration::from_secs(15);
/// Cloudflare's resolver over HTTPS: anycast, so near on any network, and
/// always listening. An address, so no DNS lookup adds to the time.
const TARGET: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(1, 1, 1, 1), 443));
/// Long enough for two SYN retransmissions on a poor link.
const TIMEOUT: Duration = Duration::from_secs(4);
/// Probes counted for loss and the latency range: five minutes of them.
const WINDOW: usize = 20;
/// Latency shown is the median of this many recent probes, about a minute,
/// so one slow handshake does not color the bar.
const LATENCY_PROBES: usize = 4;

struct Worker {
    stop: Arc<Stop>,
    results: mpsc::Receiver<Outcome>,
}

impl Drop for Worker {
    /// The thread ends after its current probe; nothing waits for it here.
    fn drop(&mut self) {
        self.stop.stop();
    }
}

fn spawn() -> Option<Worker> {
    let stop = Arc::new(Stop::default());
    let (sender, results) = mpsc::sync_channel(4);
    let shared = stop.clone();
    let spawned = thread::Builder::new()
        .name("herdr-network-quality".into())
        .spawn(move || run(&shared, &sender));
    match spawned {
        Ok(_) => Some(Worker { stop, results }),
        Err(error) => {
            tracing::warn!(category = "network-quality", %error, "could not start a latency worker");
            None
        }
    }
}

fn run(stop: &Stop, results: &mpsc::SyncSender<Outcome>) {
    loop {
        let due = Instant::now() + INTERVAL;
        let outcome = probe::run(TARGET, TIMEOUT);
        match results.try_send(outcome) {
            // The UI is behind; it will take the next one.
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => return,
        }
        if !stop.wait(due) {
            return;
        }
    }
}

/// Recent probes, oldest first, at most [`WINDOW`].
#[derive(Default)]
pub(crate) struct Reading {
    recent: VecDeque<Outcome>,
}

impl Reading {
    pub fn latest(&self) -> Option<Outcome> {
        self.recent.back().copied()
    }

    pub fn probes(&self) -> usize {
        self.recent.len()
    }

    /// The median round trip of the last [`LATENCY_PROBES`] probes that were
    /// answered; None when none of them was.
    pub fn latency(&self) -> Option<Duration> {
        let mut answered: Vec<Duration> = self
            .recent
            .iter()
            .rev()
            .take(LATENCY_PROBES)
            .filter_map(|outcome| outcome.round_trip())
            .collect();
        answered.sort_unstable();
        answered.get(answered.len() / 2).copied()
    }

    /// The fastest and slowest answered probes in the window.
    pub fn range(&self) -> Option<(Duration, Duration)> {
        let answered = self
            .recent
            .iter()
            .filter_map(|outcome| outcome.round_trip());
        let min = answered.clone().min()?;
        let max = answered.max()?;
        Some((min, max))
    }

    /// Unanswered probes as a share of the window, 0..=100.
    pub fn loss_percent(&self) -> Option<f32> {
        if self.recent.is_empty() {
            return None;
        }
        let lost = self
            .recent
            .iter()
            .filter(|outcome| outcome.round_trip().is_none())
            .count();
        Some(lost as f32 / self.recent.len() as f32 * 100.)
    }

    fn apply(&mut self, outcome: Outcome) {
        if self.recent.len() == WINDOW {
            self.recent.pop_front();
        }
        self.recent.push_back(outcome);
    }
}

#[derive(Default)]
pub(crate) struct NetworkQuality {
    /// None while hidden, or once its thread could not start or has ended.
    worker: Option<Worker>,
    /// A worker that could not start is not retried on every tick.
    started: bool,
    reading: Reading,
}

impl NetworkQuality {
    pub fn reading(&self) -> &Reading {
        &self.reading
    }

    /// Probes while `enabled`, and stops and forgets the readings once not.
    /// Returns whether anything shown changed.
    pub fn poll(&mut self, enabled: bool) -> bool {
        if !enabled {
            let had_any = self.started;
            *self = Self::default();
            return had_any;
        }
        if !self.started {
            self.started = true;
            self.worker = spawn();
        }
        let mut changed = false;
        while let Some(worker) = &self.worker {
            match worker.results.try_recv() {
                Ok(outcome) => {
                    self.reading.apply(outcome);
                    changed = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => self.worker = None,
            }
        }
        changed
    }
}
