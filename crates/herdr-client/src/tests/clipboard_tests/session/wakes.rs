use super::*;
use crate::{
    limits::QUIET_PING,
    tests::idle_wait::{idle_after, paced},
};

/// Far longer than any test runs, so only a wake or the socket ends a wait.
const NEVER: Duration = Duration::from_secs(3600);

fn ready_idle() -> (
    Client,
    Stream,
    thread::JoinHandle<Result<()>>,
    Arc<AtomicUsize>,
) {
    let (client, mut server, worker, waits) = test_client_paced(true, false, paced(NEVER, TIMEOUT));
    handshake(&mut server);
    event(&client);
    event(&client);
    (client, server, worker, waits)
}

/// Reserve a slot and return once the worker holds it and waits on it.
fn reserved(client: &Client, waits: &AtomicUsize) -> ClipboardImageUpload {
    let idle = idle_after(waits, 0);
    let upload = reserve(client);
    idle_after(waits, idle);
    upload
}

fn finishes(cancel: &ClipboardImageCancellation) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cancel.is_finished() {
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(1));
    }
    true
}

#[test]
fn publication_wakes_a_worker_waiting_on_a_prepared_slot() {
    let (client, mut server, worker, waits) = ready_idle();
    let upload = reserved(&client, &waits);
    upload
        .complete_input(ClientPaneInputEvent::Paste("late.png".into()))
        .unwrap();
    server
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    assert!(matches!(
        receive(&mut server),
        ClientMessage::ClientShellPaneInput { .. }
    ));
    client.handle.disconnect();
    worker.join().unwrap().unwrap();
}

#[test]
fn dropping_or_cancelling_a_slot_wakes_the_worker_to_release_it() {
    for drop_upload in [true, false] {
        let (client, _server, worker, waits) = ready_idle();
        let upload = reserved(&client, &waits);
        let cancel = upload.cancellation_handle();
        if drop_upload {
            drop(upload);
        } else {
            cancel.cancel();
        }
        assert!(finishes(&cancel));
        client.handle.disconnect();
        worker.join().unwrap().unwrap();
    }
}

#[test]
fn a_backpressured_image_resumes_when_the_daemon_reads() {
    let (client, mut server, worker, _) = ready_idle();
    let upload = reserve(&client);
    upload
        .complete("png", vec![7; MAX_CLIPBOARD_IMAGE_PAYLOAD])
        .unwrap();
    // The image far exceeds the socket buffer, so the worker must wait for
    // room many times; with an hour-long idle cap, only writability ends it.
    server
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let message: ClientMessage = read_message(&mut server, MAX_CLIPBOARD_IMAGE_FRAME_SIZE).unwrap();
    assert!(matches!(message, ClientMessage::ClipboardImage { .. }));
    client.handle.disconnect();
    worker.join().unwrap().unwrap();
}

#[test]
fn a_liveness_request_deferred_by_an_image_is_sent_when_the_image_ends() {
    let (client, mut server, worker, _) = test_client_paced(false, true, paced(NEVER, TIMEOUT));
    ssh_handshake(&mut server);
    event(&client);
    event(&client);
    let quiet_since = Instant::now();
    reserve(&client)
        .complete("png", vec![7; MAX_FRAME_SIZE + 1])
        .unwrap();
    let mut prefix = [0; 4];
    server.read_exact(&mut prefix).unwrap();
    // The image is now part written, so the probe must wait for its end.
    client.handle.check_liveness();
    let mut payload = vec![0; u32::from_le_bytes(prefix) as usize];
    server.read_exact(&mut payload).unwrap();
    assert!(
        matches!(receive(&mut server), ClientMessage::EndpointControl { kind, .. } if kind == "endpoint.health.ping.v1")
    );
    // Sooner than the routine ping of a quiet link, which would also send one.
    assert!(
        quiet_since.elapsed() < QUIET_PING,
        "{:?}",
        quiet_since.elapsed()
    );
    client.handle.disconnect();
    worker.join().unwrap().unwrap();
}

#[test]
fn an_upload_kept_past_its_handle_does_not_keep_the_worker_waiting() {
    let (client, _server, worker, waits) = ready_idle();
    let upload = reserved(&client, &waits);
    // The upload keeps the wake channel open, so only an explicit wake from
    // the dropped handle ends the worker's wait.
    drop(client);
    let (done, finished) = bounded(1);
    thread::spawn(move || done.send(worker.join().unwrap()));
    finished
        .recv_timeout(Duration::from_secs(10))
        .expect("the worker kept waiting")
        .unwrap();
    drop(upload);
}
