use super::*;
use crate::{
    limits::{IDLE_WAIT, QUIET_PING},
    session::wait_timeout,
};

const SOON: Duration = Duration::from_millis(100);

#[test]
fn nothing_pending_waits_the_idle_cap() {
    let now = Instant::now();
    let session = ready_session();
    assert_eq!(session.deadline(), None);
    assert_eq!(FrameReader::new().deadline(), None);
    assert_eq!(wait_timeout(now, [None, None]), IDLE_WAIT);
}

#[test]
fn each_owner_reports_the_deadline_its_check_enforces() {
    let now = Instant::now() + COMMAND_TIMEOUT * 2;

    let mut session = Session::new(true, false);
    session.started = now;
    assert_eq!(session.deadline(), Some(now + TIMEOUT));

    let mut session = ready_session();
    session.pending = Some(Pending {
        id: "1".into(),
        bytes: Vec::new(),
        started: now,
    });
    assert_eq!(session.deadline(), Some(now + COMMAND_TIMEOUT));

    let mut health = Health::new(now);
    assert_eq!(health.deadline(), now + QUIET_PING);
    health.ping = Some((now, now + SOON));
    assert_eq!(health.deadline(), now + SOON);

    let mut reader = FrameReader::new();
    reader.started = Some(now);
    assert_eq!(reader.deadline(), Some(now + TIMEOUT));
}

#[test]
fn the_wait_ends_at_the_earliest_deadline_and_never_exceeds_the_cap() {
    let now = Instant::now();
    assert_eq!(
        wait_timeout(now, [Some(now + SOON * 3), None, Some(now + SOON)]),
        SOON
    );
    assert_eq!(wait_timeout(now, [Some(now + IDLE_WAIT * 2)]), IDLE_WAIT);
    assert_eq!(wait_timeout(now + SOON, [Some(now)]), Duration::ZERO);
}

#[test]
fn an_overdue_partial_frame_is_reported_without_inbound_data() {
    let mut reader = FrameReader::new();
    reader.started = Some(Instant::now() - TIMEOUT - POLL);
    assert!(matches!(
        reader.check_timeout(),
        Err(Error::PartialFrameTimeout)
    ));
    assert!(FrameReader::new().check_timeout().is_ok());
}

#[test]
fn an_idle_worker_still_stops_promptly_on_disconnect_and_on_drop() {
    for drop_handle in [false, true] {
        let (client, mut server, worker) = test_client();
        handshake(&mut server);
        event(&client);
        event(&client);
        thread::sleep(Duration::from_millis(50));
        let asked = Instant::now();
        if drop_handle {
            drop(client);
        } else {
            client.handle.disconnect();
        }
        worker.join().unwrap().unwrap();
        assert!(asked.elapsed() < IDLE_WAIT / 2, "{:?}", asked.elapsed());
    }
}

#[test]
fn an_idle_worker_sends_a_queued_command_at_once() {
    let (client, mut server, worker) = test_client();
    handshake(&mut server);
    event(&client);
    event(&client);
    thread::sleep(Duration::from_millis(50));
    let asked = Instant::now();
    client.handle.set_focus("boot-v1", true).unwrap();
    let _ = receive(&mut server);
    assert!(
        asked.elapsed() < Duration::from_secs(1),
        "{:?}",
        asked.elapsed()
    );
    client.handle.disconnect();
    worker.join().unwrap().unwrap();
}

#[test]
fn a_frame_split_across_an_idle_gap_is_still_delivered_whole() {
    let (client, mut server, worker) = test_client();
    handshake(&mut server);
    event(&client);
    event(&client);
    let surface = baseline();
    let bytes = encode_message(
        &ServerMessage::PaneSurface(surface.clone()),
        MAX_GRAPHICS_FRAME_SIZE,
    )
    .unwrap();
    let (head, tail) = bytes.split_at(6);
    server.write_all(head).unwrap();
    thread::sleep(Duration::from_millis(100));
    server.write_all(tail).unwrap();
    assert!(matches!(event(&client), ClientEvent::Surface(s) if *s == surface));
    client.handle.disconnect();
    worker.join().unwrap().unwrap();
}
