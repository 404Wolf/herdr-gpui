use super::*;
use crate::settings_window::{controls::preferences::PreferenceIo, inputs::NumberField};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

fn quiet_load() -> crate::Result<super::super::super::Loaded> {
    let mut loaded = skill_load()?;
    loaded.config.sound.volume = 25;
    Ok(loaded)
}

/// A volume chosen in Settings applies to the next sound once its save
/// loads, without waiting for a main window to reload the file.
#[gpui::test]
fn a_saved_volume_reaches_the_shared_sound_worker_at_once(cx: &mut TestAppContext) {
    // The main window must outlive the fixture, as a real one does.
    let source = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    let weak = cx.update(|cx| source.update(cx, |_, _, cx| cx.weak_entity()).unwrap());
    let (view, cx) = cx.add_window_view(move |_, cx| SettingsWindow::new(weak.clone(), cx));
    let (sound, volumes) = crate::sound::Service::recording_volume();
    let sound = std::rc::Rc::new(sound);
    let edits = Arc::new(Mutex::new(Vec::new()));
    let captured = edits.clone();
    cx.update(|_, cx| {
        source
            .update(cx, |source, _, _| source.use_sound(sound.clone()))
            .unwrap();
    });
    view.update(cx, |view, cx| {
        view.controls.preference_io = Some(PreferenceIo {
            write: Arc::new(move |edit| {
                captured.lock().unwrap().push(edit);
                Ok(())
            }),
            load: Arc::new(quiet_load),
        });
        view.section = Section::Notifications;
        cx.notify();
    });
    cx.simulate_resize(size(px(960.), px(3200.)));
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    // Type 25 into the volume field and press Enter.
    let field = cx.debug_bounds("notif-volume-control-field").unwrap();
    assert!(field.size.width > px(0.));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.begin_number_edit(NumberField::SoundVolume, 50., window, cx)
        })
    });
    cx.simulate_input("25");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    assert_eq!(
        edits.lock().unwrap().as_slice(),
        [crate::config::preferences::Preference::SoundVolume(25)]
    );
    // The main window's own config still holds the old volume.
    cx.update(|_, cx| {
        let volume = source.read_with(cx, |source, _| source.config.sound.volume);
        assert_eq!(volume.unwrap(), 50);
    });
    sound.preview();
    assert_eq!(volumes.recv_timeout(Duration::from_secs(3)).unwrap(), 25);
}
