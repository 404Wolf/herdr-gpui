#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::{
    LATENCY_PROBES, NetworkQuality, Reading, WINDOW,
    probe::{self, Outcome},
};
use std::{
    io::ErrorKind,
    net::{SocketAddr, TcpListener},
    time::Duration,
};

fn reply(milliseconds: u64) -> Outcome {
    Outcome::Reply(Duration::from_millis(milliseconds))
}

fn lost() -> Outcome {
    Outcome::Lost(ErrorKind::TimedOut)
}

#[test]
fn a_listening_port_answers() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let target = listener.local_addr().expect("address");
    let outcome = probe::run(target, Duration::from_secs(2));
    assert!(outcome.round_trip().is_some(), "{outcome:?}");
}

#[test]
fn a_closed_port_is_lost() {
    // Bound then dropped, so nothing listens there.
    let target: SocketAddr = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("address");
    let outcome = probe::run(target, Duration::from_secs(2));
    assert_eq!(outcome, Outcome::Lost(ErrorKind::ConnectionRefused));
}

#[test]
fn latency_is_the_median_of_recent_answers() {
    let mut reading = Reading::default();
    assert_eq!(reading.latency(), None);
    // Older than the latency probes, so it does not count.
    reading.apply(reply(900));
    for milliseconds in [30, 500, 40, 35] {
        reading.apply(reply(milliseconds));
    }
    assert_eq!(reading.latency(), Some(Duration::from_millis(40)));
}

#[test]
fn latency_is_none_when_recent_probes_were_all_lost() {
    let mut reading = Reading::default();
    reading.apply(reply(30));
    for _ in 0..LATENCY_PROBES {
        reading.apply(lost());
    }
    assert_eq!(reading.latency(), None);
}

#[test]
fn loss_and_range_cover_the_window() {
    let mut reading = Reading::default();
    assert_eq!(reading.loss_percent(), None);
    reading.apply(reply(20));
    reading.apply(lost());
    reading.apply(reply(80));
    reading.apply(reply(50));
    assert_eq!(reading.loss_percent(), Some(25.));
    assert_eq!(
        reading.range(),
        Some((Duration::from_millis(20), Duration::from_millis(80)))
    );
}

#[test]
fn loss_forgets_probes_older_than_the_window() {
    let mut reading = Reading::default();
    reading.apply(lost());
    for _ in 0..WINDOW {
        reading.apply(reply(30));
    }
    assert_eq!(reading.probes(), WINDOW);
    assert_eq!(reading.loss_percent(), Some(0.));
}

#[test]
fn details_name_the_target_and_the_loss() {
    let mut reading = Reading::default();
    reading.apply(reply(31));
    reading.apply(Outcome::Lost(ErrorKind::TimedOut));
    let text = super::render::details(&reading);
    assert!(text.contains("1.1.1.1:443"), "{text}");
    assert!(text.contains("no connection"), "{text}");
    assert!(text.contains("Median 31 ms"), "{text}");
    assert!(text.contains("Loss 50% of 2 probes"), "{text}");
}

#[test]
fn hiding_stops_and_forgets() {
    let mut quality = NetworkQuality::default();
    assert!(!quality.poll(false), "nothing was shown");
    quality.reading.apply(reply(30));
    quality.started = true;
    assert!(quality.poll(false), "the reading disappears");
    assert!(quality.reading().latest().is_none());
}
