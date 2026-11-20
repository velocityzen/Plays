//! A window playing through an audio output of its own, picked from a
//! tile's menu or the speakers of the display it was sent to, as someone
//! at the screen would.

use std::sync::Arc;

use gpui::{TestAppContext, VisualTestContext, WindowHandle, px, size};

use super::open;
use crate::app::{AudioEngine, SetWindowAudioDevice, ShowOnDisplay};
use crate::media::Mixer;
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, Settings};
use crate::ui::displays;
use crate::ui::testing::{self, click, fixture, hover, shows, wait_until};
use crate::ui::workspace::Workspace;

/// Every tile's mixer in `window`, and the device it plays through.
fn sound(
    window: WindowHandle<Workspace>,
    cx: &TestAppContext,
) -> (Vec<Arc<Mixer>>, Option<String>) {
    window
        .read_with(cx, |workspace, cx| {
            let mixers = workspace
                .tiles
                .iter()
                .filter_map(|tile| tile.view.read(cx).player().mixer())
                .collect();
            let device = workspace.sound_device().map(|device| device.name.clone());
            (mixers, device)
        })
        .unwrap()
}

#[gpui::test]
fn a_window_plays_through_an_output_of_its_own(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let app = cx.update(|_, cx| AudioEngine::mixer(cx)).unwrap();
    let outputs = |cx: &mut VisualTestContext| cx.update(|_, cx| AudioEngine::window_outputs(cx));
    assert!(!shows(&mut cx, "cast"), "nothing to pick, with one output");

    cx.update(|_, cx| {
        let device = |id: &str, name: &str| DeviceInfo {
            id: id.into(),
            name: name.into(),
        };
        AudioEngine::set_devices(vec![device("desk", "Desk"), device("tv", "TV")], cx);
    });
    click(&mut cx, "cast");
    assert!(shows(&mut cx, "sound-app") && shows(&mut cx, "sound-1"));
    assert!(!shows(&mut cx, "cast-here"), "nothing else to play on");

    click(&mut cx, "sound-1");
    cx.run_until_parked();
    let (mixers, device) = sound(window, &cx);
    assert_eq!(device.as_deref(), Some("TV"));
    assert!(!Arc::ptr_eq(&mixers[0], &app), "the tile plays through it");
    assert_eq!(outputs(&mut cx), 1);
    let tv = mixers[0].clone();

    // A file opened in it plays there too.
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![fixture("tone.m4a")], cx)
        })
        .unwrap();
    let opened = wait_until(&mut cx, |cx| sound(window, cx).0.len() == 2);
    assert!(opened, "tone.m4a didn't open");
    let (mixers, _) = sound(window, &cx);
    assert!(mixers.iter().all(|mixer| Arc::ptr_eq(mixer, &tv)));

    // A tile popped out takes the device along, sharing its output.
    let before = cx.windows();
    hover(&mut cx, "tile-0");
    click(&mut cx, "pop-out");
    cx.run_until_parked();
    let popped = cx
        .windows()
        .into_iter()
        .find(|window| !before.contains(window))
        .and_then(|window| window.downcast::<Workspace>())
        .expect("a window for the tile");
    let (mixers, device) = sound(popped, &cx);
    assert_eq!(device.as_deref(), Some("TV"));
    assert!(Arc::ptr_eq(&mixers[0], &tv));
    assert_eq!(outputs(&mut cx), 1, "one output for both");

    // Back to the app's output: the other window keeps the device.
    cx.dispatch_action(SetWindowAudioDevice(None));
    cx.run_until_parked();
    let (mixers, device) = sound(window, &cx);
    assert_eq!(device, None);
    assert!(Arc::ptr_eq(&mixers[0], &app));
    assert_eq!(outputs(&mut cx), 1);

    // Its last window closed, the output closes.
    popped
        .update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(outputs(&mut cx), 0);
}

/// Whether the window is full screen and away on another display, once
/// it has drawn.
fn away(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    window
        .read_with(cx, |workspace, _| workspace.is_away())
        .unwrap()
}

/// Have Output Device send the window full screen to display `to`.
#[track_caller]
fn send(window: WindowHandle<Workspace>, to: u32, cx: &mut VisualTestContext) {
    cx.dispatch_action(ShowOnDisplay(to));
    assert!(away(window, cx), "full screen on the other display");
}

/// Leave full screen, which brings a window Output Device sent away back.
#[track_caller]
fn come_back(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) {
    cx.update(|window, _| window.toggle_fullscreen());
    assert!(!away(window, cx), "back where it was");
}

#[gpui::test]
fn a_window_sent_to_a_display_plays_through_its_speakers_until_it_comes_back(
    cx: &mut TestAppContext,
) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let app = cx.update(|_, cx| AudioEngine::mixer(cx)).unwrap();
    // Two TVs, whose HDMI outputs are named after them, and a display
    // without speakers. (The test window stays on a display it was sent
    // to once it is back, as it has no frame to move.)
    let connect = || u32::from(cx.add_display(size(px(1920.), px(1080.))));
    let (den, attic, plain) = (connect(), connect(), connect());
    let named = |id: u32, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let display = displays::list(cx).into_iter().find(|d| d.id == id);
            display.expect("a display").name
        })
    };
    let (den_tv, attic_tv) = (named(den, &mut cx), named(attic, &mut cx));
    cx.update(|_, cx| {
        let device = |id: &str, name: &str| DeviceInfo {
            id: id.into(),
            name: name.into(),
        };
        let devices = vec![
            device("desk", "Desk"),
            device("den", &den_tv),
            device("attic", &attic_tv),
        ];
        AudioEngine::set_devices(devices, cx);
    });
    let playing_through = |cx: &mut VisualTestContext| sound(window, cx).1;

    send(window, den, &mut cx);
    let (mixers, device) = sound(window, &cx);
    assert_eq!(device.as_ref(), Some(&den_tv), "the TV's speakers");
    assert!(!Arc::ptr_eq(&mixers[0], &app));
    // Sent on, it follows; to a display without speakers, it plays as it
    // did.
    send(window, attic, &mut cx);
    assert_eq!(playing_through(&mut cx).as_ref(), Some(&attic_tv));
    send(window, plain, &mut cx);
    assert_eq!(playing_through(&mut cx), None);
    send(window, den, &mut cx);
    assert_eq!(playing_through(&mut cx).as_ref(), Some(&den_tv));
    come_back(window, &mut cx);
    let (mixers, device) = sound(window, &cx);
    assert_eq!(device, None, "the app's output again");
    assert!(Arc::ptr_eq(&mixers[0], &app));
    assert_eq!(cx.update(|_, cx| AudioEngine::window_outputs(cx)), 0);

    // Full screen where it is, the sound stays.
    let here = cx.update(|window, cx| displays::of(window, cx)).unwrap();
    cx.dispatch_action(ShowOnDisplay(here));
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.is_fullscreen()));
    assert_eq!(playing_through(&mut cx), None);
    cx.update(|window, _| window.toggle_fullscreen());
    cx.run_until_parked();

    // An output picked for it, it keeps.
    cx.dispatch_action(SetWindowAudioDevice(Some(AudioDevice {
        id: "desk".into(),
        name: "Desk".into(),
    })));
    send(window, attic, &mut cx);
    assert_eq!(playing_through(&mut cx).as_deref(), Some("Desk"));
    come_back(window, &mut cx);
    assert_eq!(playing_through(&mut cx).as_deref(), Some("Desk"));

    // An output picked while there, it keeps coming back.
    cx.dispatch_action(SetWindowAudioDevice(None));
    send(window, den, &mut cx);
    assert_eq!(playing_through(&mut cx).as_ref(), Some(&den_tv));
    cx.dispatch_action(SetWindowAudioDevice(Some(AudioDevice {
        id: "desk".into(),
        name: "Desk".into(),
    })));
    cx.run_until_parked();
    come_back(window, &mut cx);
    assert_eq!(playing_through(&mut cx).as_deref(), Some("Desk"));

    // Settings say not to.
    cx.dispatch_action(SetWindowAudioDevice(None));
    cx.update(|_, cx| Settings::update(cx, false, |s| s.sound_follows_display = false));
    send(window, attic, &mut cx);
    assert_eq!(playing_through(&mut cx), None);
    come_back(window, &mut cx);
    assert_eq!(playing_through(&mut cx), None);
}
