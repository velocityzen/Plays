use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;

use gpui::TestAppContext;

use super::*;
use crate::ui::testing::wait_until;

/// Whether the devices `open_slowly` opens are ready: each waits for
/// it, or for `SLOW` at most.
static READY: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
const SLOW: Duration = Duration::from_secs(5);
/// The threads devices were opened on.
static OPENED_ON: Mutex<Vec<ThreadId>> = Mutex::new(Vec::new());

/// Opens any device, once it is ready, as some take a while to.
fn open_slowly(_: &mut AudioOutput, id: Option<&str>, _: OnEvent) -> Result<Playing, OpenError> {
    OPENED_ON.lock().unwrap().push(std::thread::current().id());
    let (ready, readied) = &READY;
    let ready = ready.lock().unwrap();
    drop(readied.wait_timeout_while(ready, SLOW, |ready| !*ready));
    Ok(Playing {
        id: id.map(str::to_owned),
        name: id.unwrap_or("Speakers").into(),
        rate: 44_100,
        channels: 2,
    })
}

fn set_ready(now: bool) {
    let (ready, readied) = &READY;
    *ready.lock().unwrap() = now;
    readied.notify_all();
}

fn playing(cx: &TestAppContext) -> Option<String> {
    cx.read(|cx| match AudioEngine::route(cx) {
        Some(Route::Device { playing, .. }) => Some(playing.name.clone()),
        _ => None,
    })
}

#[gpui::test]
fn opens_devices_without_the_app_waiting_for_them(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_global(Settings::default());
        AudioEngine::start_with(
            Devices {
                local: Local {
                    list: Vec::new,
                    is_connected: |_| true,
                    open: open_slowly,
                    watch: drop,
                },
                bluetooth: Bluetooth::NONE,
                speakers: None,
            },
            cx,
        );
    });
    assert_eq!(playing(cx), None, "the app went on while it opened");
    set_ready(true);
    assert!(wait_until(cx, |cx| playing(cx).is_some()));
    assert_eq!(playing(cx).as_deref(), Some("Speakers"));

    set_ready(false);
    cx.update(|cx| {
        Settings::update(cx, false, |settings| {
            settings.audio_output.device = Some(AudioDevice {
                id: "desk".into(),
                name: "Desk".into(),
            });
        });
    });
    assert_eq!(playing(cx), None, "the app went on while it opened");
    let shown = cx.read(AudioEngine::device_description);
    assert_eq!(shown, "Opening Desk…");
    set_ready(true);
    assert!(wait_until(cx, |cx| playing(cx).is_some()));
    assert_eq!(playing(cx).as_deref(), Some("desk"));

    let ui = std::thread::current().id();
    let opened_on = OPENED_ON.lock().unwrap();
    assert_eq!(opened_on.len(), 2);
    assert!(
        opened_on.iter().all(|&thread| thread != ui),
        "opened on the UI thread"
    );
}
