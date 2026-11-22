//! A window playing through an audio output of its own, picked from a
//! tile's menu or the speakers of the display it was sent to, as someone
//! at the screen would.

use std::sync::Arc;

use gpui::{TestAppContext, VisualTestContext, WindowHandle, px, size};

use super::*;
use crate::api::commands::output;
use crate::api::commands::window::show_on_display;
use crate::app::AudioEngine;
use crate::cast::fake::FakeTarget;
use crate::cast::{Casting, Shows};
use crate::media::Mixer;
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, Settings};
use crate::ui::displays;
use crate::ui::testing::{self, click, fixture, hover, painted, shows, wait_until};
use crate::ui::workspace::Workspace;
use crate::ui::workspace::tests::open;

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
fn a_speaker_found_with_airplay_and_dlna_shows_under_both_saying_which(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let fake = FakeTarget::default();
    fake.add("Phantom", Shows::Sound);
    cx.update(|_, cx| {
        cx.set_global(Casting::local(vec![Arc::new(fake.clone())]));
        let device = |id: &str, name: &str| DeviceInfo {
            id: id.into(),
            name: name.into(),
        };
        let outputs = vec![device("desk", "Desk"), device("airplay:ph", "Phantom")];
        AudioEngine::set_devices(outputs, cx);
    });
    click(&mut cx, "cast");
    // Cast to, saying its kind, under what the tile plays on, and as an
    // output under what the window's sound plays on, saying AirPlay.
    let headings: Vec<_> = painted(&mut cx)
        .into_iter()
        .map(|(text, _)| text)
        .filter(|text| text.starts_with("Play This"))
        .collect();
    assert_eq!(
        headings,
        ["Play This Tile On", "Play This Window's Sound On"]
    );
    assert!(shows(&mut cx, "cast-0"), "the speaker to cast to");
    assert!(shows(&mut cx, "sound-1"), "the AirPlay speaker");
    assert!(shows(&mut cx, "sound-1-kind"), "saying it's AirPlay");
    assert!(
        !shows(&mut cx, "sound-0-kind"),
        "this computer's own says nothing"
    );
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
    assert!(
        painted(&mut cx)
            .iter()
            .any(|(text, _)| *text == "Same as the App"),
        "as the palette and Audio → Window Output Device say"
    );
    assert!(!shows(&mut cx, "cast-here"), "nothing else to play on");
    assert!(!shows(&mut cx, "cast-selected"), "it plays as the app does");

    click(&mut cx, "sound-1");
    cx.run_until_parked();
    let (mixers, device) = sound(window, &cx);
    assert_eq!(device.as_deref(), Some("TV"));
    assert!(shows(&mut cx, "cast-selected"), "lit, playing elsewhere");
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
    cx.dispatch_action(output::set_window_device(None));
    cx.run_until_parked();
    let (mixers, device) = sound(window, &cx);
    assert_eq!(device, None);
    assert!(!shows(&mut cx, "cast-selected"));
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
    cx.dispatch_action(show_on_display(to));
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
    cx.dispatch_action(show_on_display(here));
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.is_fullscreen()));
    assert_eq!(playing_through(&mut cx), None);
    cx.update(|window, _| window.toggle_fullscreen());
    cx.run_until_parked();

    // An output picked for it, it keeps.
    cx.dispatch_action(output::set_window_device(Some(&AudioDevice {
        id: "desk".into(),
        name: "Desk".into(),
    })));
    send(window, attic, &mut cx);
    assert_eq!(playing_through(&mut cx).as_deref(), Some("Desk"));
    come_back(window, &mut cx);
    assert_eq!(playing_through(&mut cx).as_deref(), Some("Desk"));

    // An output picked while there, it keeps coming back.
    cx.dispatch_action(output::set_window_device(None));
    send(window, den, &mut cx);
    assert_eq!(playing_through(&mut cx).as_ref(), Some(&den_tv));
    cx.dispatch_action(output::set_window_device(Some(&AudioDevice {
        id: "desk".into(),
        name: "Desk".into(),
    })));
    cx.run_until_parked();
    come_back(window, &mut cx);
    assert_eq!(playing_through(&mut cx).as_deref(), Some("Desk"));

    // Settings say not to.
    cx.dispatch_action(output::set_window_device(None));
    cx.update(|_, cx| Settings::update(cx, false, |s| s.sound_follows_display = false));
    send(window, attic, &mut cx);
    assert_eq!(playing_through(&mut cx), None);
    come_back(window, &mut cx);
    assert_eq!(playing_through(&mut cx), None);
}

// Which output is a display's speakers, without a window.

fn outputs(names: &[&str]) -> Vec<DeviceInfo> {
    names
        .iter()
        .enumerate()
        .map(|(index, name)| DeviceInfo {
            id: format!("output-{index}"),
            name: (*name).to_owned(),
        })
        .collect()
}

fn speakers(display: &str, names: &[&str]) -> Option<String> {
    speakers_of(display, &outputs(names)).map(|device| device.name.clone())
}

#[test]
fn a_displays_speakers_are_the_output_named_after_it() {
    let mac = ["MacBook Pro Speakers", "AirPods Pro", "LG TV SSCR2"];
    assert_eq!(
        speakers("LG TV SSCR2", &mac).as_deref(),
        Some("LG TV SSCR2")
    );
    assert_eq!(
        speakers("lg tv  sscr2", &mac).as_deref(),
        Some("LG TV SSCR2")
    );
    assert_eq!(
        speakers("Built-in Retina Display", &mac),
        None,
        "none of its own"
    );

    assert_eq!(
        speakers("Studio Display", &["Studio Display Speakers", "AirPods"]).as_deref(),
        Some("Studio Display Speakers"),
    );
    assert_eq!(
        speakers("DELL U2720Q (DP-1)", &["DELL U2720Q", "Headphones"]).as_deref(),
        Some("DELL U2720Q"),
        "a display named with its connector too",
    );
    assert_eq!(
        speakers("LG TV", &["LG TV SSCR2", "LG TV Soundbar"]),
        None,
        "not one of two",
    );
    assert_eq!(
        speakers("LG TV", &["LG TV", "LG TV Soundbar"]).as_deref(),
        Some("LG TV"),
        "the one named as it is, first",
    );
    assert_eq!(speakers("TV", &["TVS Speakers"]), None, "word for word");
    assert_eq!(speakers("", &["Speakers"]), None);
}
