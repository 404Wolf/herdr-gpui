use super::*;

/// A worker that reports the volume each sound would play at.
fn recording_volume() -> (Service, mpsc::Receiver<u8>) {
    let (sender, receiver) = mpsc::sync_channel(8);
    let service = Service::start(
        || Ok(Settings::default()),
        move |_, _, volume, _, _| {
            let _ = sender.try_send(volume);
            Ok(())
        },
    );
    (service, receiver)
}

#[test]
fn sounds_start_at_the_default_volume() {
    let (service, volumes) = recording_volume();
    service.preview();
    assert_eq!(
        volumes.recv_timeout(Duration::from_secs(3)).unwrap(),
        crate::config::SoundConfig::default().volume
    );
}

#[test]
fn each_sound_plays_at_the_volume_set_before_it() {
    let (service, volumes) = recording_volume();
    for percent in [0, 30, 100] {
        service.set_volume(percent);
        service.preview();
        assert_eq!(
            volumes.recv_timeout(Duration::from_secs(3)).unwrap(),
            percent
        );
    }
}
