//! A block playing through an audio output of its own, picked from its
//! menu, and a window through the speakers of the display it was sent to,
//! as someone at the screen would.

use std::sync::Arc;

use gpui::{TestAppContext, VisualTestContext, WindowHandle, px, size};

use super::*;
use crate::api::commands::output;
use crate::api::commands::window::show_on_display;
use crate::api::{self, Invocation};
use crate::app::AudioEngine;
use crate::cast::fake::FakeTarget;
use crate::cast::{Casting, Shows};
use crate::media::Mixer;
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, Settings};
use crate::ui::displays;
use crate::ui::testing::{self, checked, click, find, fixture, hover, painted, shows, wait_until};
use crate::ui::workspace::Workspace;
use crate::ui::workspace::tests::open;

/// Every block's mixer in `window`, and the device each plays through: its
/// own, or the speakers of the display the window was sent to.
fn sound(
    window: WindowHandle<Workspace>,
    cx: &TestAppContext,
) -> (Vec<Arc<Mixer>>, Vec<Option<String>>) {
    window
        .read_with(cx, |workspace, cx| {
            let display = workspace.display_sound().map(|device| device.name.clone());
            workspace
                .blocks
                .iter()
                .filter_map(|block| {
                    let view = block.view.read(cx);
                    let own = view.sound_device().map(|device| device.name.clone());
                    Some((view.player().mixer()?, own.or_else(|| display.clone())))
                })
                .unzip()
        })
        .unwrap()
}

/// The outputs `outputs` lists, by id and name, as connected.
fn connected(outputs: &[(&str, &str)], cx: &mut VisualTestContext) {
    let devices = outputs
        .iter()
        .map(|&(id, name)| DeviceInfo {
            id: id.into(),
            name: name.into(),
            bluetooth: false,
        })
        .collect();
    cx.update(|_, cx| AudioEngine::set_devices(devices, cx));
}

/// The headings of the block's menu, top to bottom.
fn headings(cx: &mut VisualTestContext) -> Vec<String> {
    let mut headings: Vec<_> = painted(cx)
        .into_iter()
        .filter(|(text, _)| text.starts_with("Play "))
        .collect();
    headings.sort_by(|a, b| f32::from(a.1.origin.y).total_cmp(&f32::from(b.1.origin.y)));
    headings
        .into_iter()
        .map(|(text, _)| text.to_string())
        .collect()
}

/// Whether the row tagged `selector` is under the heading `heading`, the
/// last above it.
fn under(cx: &mut VisualTestContext, heading: &str, selector: &'static str) -> bool {
    let row = find(cx, selector).origin.y;
    painted(cx)
        .into_iter()
        .filter(|(text, bounds)| text.starts_with("Play ") && bounds.origin.y < row)
        .max_by(|a, b| f32::from(a.1.origin.y).total_cmp(&f32::from(b.1.origin.y)))
        .is_some_and(|(text, _)| text == heading)
}

#[gpui::test]
fn a_blocks_menu_says_where_its_picture_and_its_sound_play(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let fake = FakeTarget::default();
    let tv = fake.add("Den TV", Shows::Video);
    // A speaker cast to, whose id an AirPlay output's has too.
    let phantom = fake.add("Phantom", Shows::Sound);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
    let airplay = format!("airplay:{}", phantom.id);
    connected(&[("desk", "Desk"), (&airplay, "Phantom")], &mut cx);
    click(&mut cx, "cast");
    assert_eq!(headings(&mut cx), ["Play Video On", "Play Audio On"]);
    assert!(under(&mut cx, "Play Video On", "cast-here"));
    assert!(under(&mut cx, "Play Video On", "cast-0"), "the TV");
    assert!(under(&mut cx, "Play Audio On", "sound-app"));
    assert!(under(&mut cx, "Play Audio On", "cast-1"), "the speaker");
    // As an output too, saying AirPlay, as the speaker cast to says DLNA.
    assert!(under(&mut cx, "Play Audio On", "sound-1"));
    assert!(shows(&mut cx, "sound-1-kind"));
    assert!(
        !shows(&mut cx, "sound-0-kind"),
        "this computer's own says nothing"
    );
    assert!(checked(&mut cx, "cast-here") && checked(&mut cx, "sound-app"));

    // Cast with its sound: that's where both play.
    let view = window
        .read_with(&cx, |workspace, _| workspace.blocks[0].view.clone())
        .unwrap();
    view.update(&mut cx, |view, cx| view.cast_to(Some(tv.clone()), cx));
    cx.run_until_parked();
    click(&mut cx, "cast");
    assert!(checked(&mut cx, "cast-0") && !checked(&mut cx, "cast-here"));
    assert!(under(&mut cx, "Play Audio On", "sound-cast"));
    assert!(checked(&mut cx, "sound-cast"), "the TV plays its sound");
    assert!(!checked(&mut cx, "sound-app") && !checked(&mut cx, "sound-0"));

    // An output picked plays it here, through that.
    click(&mut cx, "sound-0");
    cx.run_until_parked();
    assert!(view.read_with(&cx, |view, _| view.casting().is_none()));
    assert_eq!(sound(window, &cx).1, [Some("Desk".into())]);
    click(&mut cx, "cast");
    assert!(checked(&mut cx, "sound-0") && !checked(&mut cx, "sound-app"));
    click(&mut cx, "cast");

    // Its sound cast to the speaker: that's checked, not the output of
    // another kind with its id, nor its own.
    view.update(&mut cx, |view, cx| view.cast_to(Some(phantom.clone()), cx));
    cx.run_until_parked();
    click(&mut cx, "cast");
    assert!(checked(&mut cx, "cast-1") && checked(&mut cx, "cast-here"));
    assert!(!checked(&mut cx, "sound-1") && !checked(&mut cx, "sound-0"));
    assert!(!shows(&mut cx, "sound-cast"), "listed once");
}

#[gpui::test]
fn a_bluetooth_output_says_so_on_a_blocks_menu(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["tone.m4a"], false);
    let output = |id: &str, name: &str, bluetooth| DeviceInfo {
        id: id.into(),
        name: name.into(),
        bluetooth,
    };
    let devices = vec![
        output("coreaudio:BuiltInSpeakerDevice", "Speakers", false),
        output("coreaudio:00-00-5E-00-53-01:output", "Radio", true),
    ];
    cx.update(|_, cx| AudioEngine::set_devices(devices, cx));
    click(&mut cx, "cast");
    // What's written on the row tagged `row`.
    let says = |cx: &mut VisualTestContext, row: &'static str| {
        let row = find(cx, row);
        painted(cx)
            .into_iter()
            .filter(|(_, bounds)| row.contains(&bounds.center()))
            .map(|(text, _)| text.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(says(&mut cx, "sound-1"), ["Radio", "Bluetooth"]);
    assert!(shows(&mut cx, "sound-1-kind"));
    assert_eq!(says(&mut cx, "sound-0"), ["Speakers"], "built in, nothing");
    assert!(!shows(&mut cx, "sound-0-kind"));
}

#[gpui::test]
fn a_blocks_picture_cast_checks_no_display(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    cx.add_display(size(px(1920.), px(1080.)));
    let fake = FakeTarget::default();
    let tv = fake.add("Den TV", Shows::Video);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
    click(&mut cx, "cast");
    assert!(checked(&mut cx, "display-0") != checked(&mut cx, "display-1"));
    click(&mut cx, "cast");

    let view = window
        .read_with(&cx, |workspace, _| workspace.blocks[0].view.clone())
        .unwrap();
    view.update(&mut cx, |view, cx| view.cast_to(Some(tv.clone()), cx));
    cx.run_until_parked();
    click(&mut cx, "cast");
    assert!(checked(&mut cx, "cast-0"));
    assert!(!checked(&mut cx, "display-0") && !checked(&mut cx, "display-1"));
}

#[gpui::test]
fn a_sound_files_menu_offers_every_device_for_its_sound(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["tone.m4a"], false);
    let fake = FakeTarget::default();
    fake.add("Den TV", Shows::Video);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
    click(&mut cx, "cast");
    assert_eq!(headings(&mut cx), ["Play Audio On"], "no picture to play");
    assert!(under(&mut cx, "Play Audio On", "cast-0"), "the TV plays it");
}

#[gpui::test]
fn the_app_playing_through_an_airplay_speaker_shows_on_a_blocks_menu(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let beamer = AudioDevice {
        id: "airplay:tv".into(),
        name: "Beamer".into(),
    };
    connected(&[("desk", "Desk"), ("airplay:tv", "Beamer")], &mut cx);
    click(&mut cx, "cast");
    assert!(!shows(&mut cx, "cast-selected"), "it plays here");
    let says = |cx: &mut VisualTestContext| {
        let output = find(cx, "sound-app-output");
        painted(cx)
            .into_iter()
            .find(|(_, bounds)| output.contains(&bounds.center()))
            .map(|(text, _)| text.to_string())
    };
    assert_eq!(says(&mut cx).as_deref(), Some("System Default"));

    // The menu still open.
    cx.dispatch_action(output::set_device(Some(&beamer)));
    cx.run_until_parked();
    assert!(shows(&mut cx, "cast-selected"), "lit, playing elsewhere");
    assert_eq!(says(&mut cx).as_deref(), Some("Beamer"));
    assert!(checked(&mut cx, "sound-app"), "as the app plays");
    let _ = window;
}

#[gpui::test]
fn a_block_plays_through_an_output_of_its_own(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let app = cx.update(|_, cx| AudioEngine::mixer(cx)).unwrap();
    let outputs = |cx: &mut VisualTestContext| cx.update(|_, cx| AudioEngine::other_outputs(cx));
    assert!(!shows(&mut cx, "cast"), "nothing to pick, with one output");

    connected(&[("desk", "Desk"), ("tv", "TV")], &mut cx);
    click(&mut cx, "cast");
    assert!(shows(&mut cx, "sound-app") && shows(&mut cx, "sound-1"));
    assert!(
        painted(&mut cx)
            .iter()
            .any(|(text, _)| *text == "Same as the App"),
        "as the palette and Audio → Block Output Device say"
    );
    assert!(!shows(&mut cx, "cast-here"), "nothing else to play on");
    assert!(!shows(&mut cx, "cast-selected"), "it plays as the app does");

    click(&mut cx, "sound-1");
    cx.run_until_parked();
    let (mixers, devices) = sound(window, &cx);
    assert_eq!(devices, [Some("TV".into())]);
    assert!(shows(&mut cx, "cast-selected"), "lit, playing elsewhere");
    assert!(!Arc::ptr_eq(&mixers[0], &app), "the block plays through it");
    assert_eq!(outputs(&mut cx), 1);
    let tv = mixers[0].clone();

    // Gone, it's listed still, checked, as the block waits for it.
    connected(&[("desk", "Desk")], &mut cx);
    click(&mut cx, "cast");
    assert!(checked(&mut cx, "sound-missing"));
    click(&mut cx, "cast");
    connected(&[("desk", "Desk"), ("tv", "TV")], &mut cx);

    // A file opened beside it plays as the app does: only the block picked
    // plays through it.
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![fixture("tone.m4a")], cx)
        })
        .unwrap();
    let opened = wait_until(&mut cx, |cx| sound(window, cx).0.len() == 2);
    assert!(opened, "tone.m4a didn't open");
    let (mixers, devices) = sound(window, &cx);
    assert!(Arc::ptr_eq(&mixers[0], &tv) && Arc::ptr_eq(&mixers[1], &app));
    assert_eq!(devices, [Some("TV".into()), None]);

    // Picked for it too, it shares the output.
    let name = |window: WindowHandle<Workspace>, at: usize, cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, cx| {
                workspace.blocks[at].view.read(cx).id().to_string()
            })
            .unwrap()
    };
    let (clip, tone) = (name(window, 0, &cx), name(window, 1, &cx));
    let pick = |block: &str, args| {
        Invocation::new("output.set_block_device")
            .with_args(args)
            .on_block(block)
    };
    let run = pick(&tone, serde_json::json!({"id": "tv"}));
    TestAppContext::update(&cx, |cx| api::run(run, cx)).unwrap();
    cx.run_until_parked();
    let (mixers, _) = sound(window, &cx);
    assert!(mixers.iter().all(|mixer| Arc::ptr_eq(mixer, &tv)));
    assert_eq!(outputs(&mut cx), 1, "one output for both");

    // A block popped out takes its device along.
    let before = cx.windows();
    hover(&mut cx, "block-0");
    click(&mut cx, "pop-out");
    cx.run_until_parked();
    let popped = cx
        .windows()
        .into_iter()
        .find(|window| !before.contains(window))
        .and_then(|window| window.downcast::<Workspace>())
        .expect("a window for the block");
    assert_eq!(name(popped, 0, &cx), tone);
    let (mixers, devices) = sound(popped, &cx);
    assert_eq!(devices, [Some("TV".into())]);
    assert!(Arc::ptr_eq(&mixers[0], &tv));
    assert_eq!(outputs(&mut cx), 1);

    // The one left back to the app's output: the other keeps the device.
    let run = pick(&clip, serde_json::Value::Null);
    TestAppContext::update(&cx, |cx| api::run(run, cx)).unwrap();
    cx.run_until_parked();
    let (mixers, devices) = sound(window, &cx);
    assert_eq!(devices, [None]);
    assert!(Arc::ptr_eq(&mixers[0], &app));
    assert_eq!(outputs(&mut cx), 1);

    // Its last block closed, the output closes.
    popped
        .update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(outputs(&mut cx), 0);
}

#[gpui::test]
fn a_block_closed_lets_go_of_its_output(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4", "tone.m4a"], false);
    connected(&[("desk", "Desk"), ("tv", "TV")], &mut cx);
    let outputs = |cx: &mut VisualTestContext| cx.update(|_, cx| AudioEngine::other_outputs(cx));
    let names: Vec<String> = window
        .read_with(&cx, |workspace, cx| {
            let views = workspace.blocks.iter().map(|block| block.view.read(cx));
            views.map(|view| view.id().to_string()).collect()
        })
        .unwrap();
    let run = |cx: &mut VisualTestContext, invocation: Invocation| {
        TestAppContext::update(cx, |cx| api::run(invocation, cx)).unwrap();
        cx.run_until_parked();
    };
    let pick = |block: &str| {
        Invocation::new("output.set_block_device")
            .with_args(serde_json::json!({"id": "tv"}))
            .on_block(block)
    };

    run(&mut cx, pick(&names[0]));
    assert_eq!(outputs(&mut cx), 1);
    run(&mut cx, Invocation::new("block.close").on_block(&names[0]));
    assert_eq!(outputs(&mut cx), 0, "closed with its block");

    run(&mut cx, pick(&names[1]));
    assert_eq!(outputs(&mut cx), 1);
    run(&mut cx, Invocation::new("block.close_all"));
    assert_eq!(outputs(&mut cx), 0, "closed with every block");
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
            bluetooth: false,
        };
        let devices = vec![
            device("desk", "Desk"),
            device("den", &den_tv),
            device("attic", &attic_tv),
        ];
        AudioEngine::set_devices(devices, cx);
    });
    let playing_through = |cx: &mut VisualTestContext| sound(window, cx).1[0].clone();

    send(window, den, &mut cx);
    let (mixers, devices) = sound(window, &cx);
    assert_eq!(devices[0].as_ref(), Some(&den_tv), "the TV's speakers");
    assert!(!Arc::ptr_eq(&mixers[0], &app));
    // Sent on, it follows; to a display without speakers, it plays as it
    // did.
    send(window, attic, &mut cx);
    assert_eq!(playing_through(&mut cx).as_ref(), Some(&attic_tv));
    send(window, plain, &mut cx);
    assert_eq!(playing_through(&mut cx), None);
    send(window, den, &mut cx);
    assert_eq!(playing_through(&mut cx).as_ref(), Some(&den_tv));
    // Its speakers unpicked, as a TV forgotten is, the app's output.
    // Outside the window's update, as the app does.
    TestAppContext::update(&cx, |cx| AudioEngine::unpick("den", cx));
    cx.run_until_parked();
    assert_eq!(playing_through(&mut cx), None);
    come_back(window, &mut cx);
    let (mixers, devices) = sound(window, &cx);
    assert_eq!(devices, [None], "the app's output again");
    assert!(Arc::ptr_eq(&mixers[0], &app));
    assert_eq!(cx.update(|_, cx| AudioEngine::other_outputs(cx)), 0);

    // Full screen where it is, the sound stays.
    let here = cx.update(|window, cx| displays::of(window, cx)).unwrap();
    cx.dispatch_action(show_on_display(here));
    cx.run_until_parked();
    assert!(cx.update(|window, _| window.is_fullscreen()));
    assert_eq!(playing_through(&mut cx), None);
    cx.update(|window, _| window.toggle_fullscreen());
    cx.run_until_parked();

    // An output picked for it, it keeps.
    cx.dispatch_action(output::set_block_device(Some(&AudioDevice {
        id: "desk".into(),
        name: "Desk".into(),
    })));
    send(window, attic, &mut cx);
    assert_eq!(playing_through(&mut cx).as_deref(), Some("Desk"));
    come_back(window, &mut cx);
    assert_eq!(playing_through(&mut cx).as_deref(), Some("Desk"));

    // An output picked while there, it keeps coming back.
    cx.dispatch_action(output::set_block_device(None));
    send(window, den, &mut cx);
    assert_eq!(playing_through(&mut cx).as_ref(), Some(&den_tv));
    cx.dispatch_action(output::set_block_device(Some(&AudioDevice {
        id: "desk".into(),
        name: "Desk".into(),
    })));
    cx.run_until_parked();
    come_back(window, &mut cx);
    assert_eq!(playing_through(&mut cx).as_deref(), Some("Desk"));

    // Settings say not to.
    cx.dispatch_action(output::set_block_device(None));
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
            bluetooth: false,
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
