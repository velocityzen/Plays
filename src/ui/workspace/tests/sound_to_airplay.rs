//! Playing the sound through the AirPlay crate's fake speakers, picked as
//! someone at the screen would, as the app's output or a block's: found
//! on the network beside this computer's own, played through, gone and
//! back, paired with the PIN a TV shows when it wants one, let go of
//! while a block is cast to it, and forgotten.

use std::sync::Arc;

use airplay::fake::{FakeReceiver, Kind};
use gpui::{TestAppContext, VisualTestContext, WindowHandle};

use super::casting::wait;
use super::casting_to_airplay::{sheet_says, wants_pin};
use super::open;
use crate::api::commands::output;
use crate::api::{self, Invocation};
use crate::app::{AudioEngine, Bluetooth, Devices, Local, Route, Speakers};
use crate::cast::airplay::AirPlayTarget;
use crate::cast::{AppPairings, Casting};
use crate::media::AudioOutput;
use crate::media::output::{OpenError, Playing};
use crate::settings::{AudioDevice, Settings};
use crate::ui::testing::{self, click, open_settings, press, shows};
use crate::ui::workspace::Workspace;

const PIN: &str = "4321";
const HERE: &str = "This computer";

/// This computer's one output, its default, which plays nowhere.
fn this_computer(output: &mut AudioOutput, id: Option<&str>) -> Result<Playing, OpenError> {
    if id.is_some() {
        return Err(OpenError::Missing);
    }
    output.play_nowhere();
    Ok(Playing {
        id: None,
        name: HERE.into(),
        rate: 48_000,
        channels: 2,
    })
}

/// The app with `receivers` found on the network beside this computer's
/// output, waiting for a device picked while it's gone or not, as
/// `wait_for_device` says, and a tone playing over and over in a block.
fn start(
    cx: &mut TestAppContext,
    receivers: &[&FakeReceiver],
    wait_for_device: bool,
) -> (WindowHandle<Workspace>, VisualTestContext, Arc<Speakers>) {
    let (window, mut cx) = testing::start(cx);
    let found = receivers.iter().map(|receiver| receiver.device()).collect();
    let speakers = cx.update(|_, cx| {
        let pairings = Arc::clone(&cx.global::<AppPairings>().0);
        Arc::new(Speakers::with(found, pairings))
    });
    let devices = Devices {
        local: Local {
            list: Vec::new,
            is_connected: |id| id.is_none(),
            open: |output, id, _| this_computer(output, id),
            watch: drop,
        },
        bluetooth: Bluetooth::NONE,
        speakers: Some(Arc::clone(&speakers)),
    };
    cx.update(|_, cx| {
        Settings::update(cx, false, |settings| {
            settings.audio_output.wait_for_device = wait_for_device;
            // The tone is a second long: it goes on as long as a test does.
            settings.loop_by_default = true;
        });
        AudioEngine::start_with(devices, cx);
    });
    open(window, &mut cx, &["tone.m4a"], true);
    (window, cx, speakers)
}

/// What the app's sound goes through, by name, and the device picked it
/// stands in for.
fn playing(cx: &mut VisualTestContext) -> Option<(String, Option<String>)> {
    cx.update(|_, cx| match AudioEngine::route(cx) {
        Some(Route::Device { playing, missing }) => Some((playing.name.clone(), missing.clone())),
        _ => None,
    })
}

/// Whether the app's sound goes through `name`, picked.
fn plays_through(cx: &mut VisualTestContext, name: &str) -> bool {
    playing(cx).is_some_and(|(playing, missing)| playing == name && missing.is_none())
}

/// Whether `receiver` was sent sound, not silence.
fn heard(receiver: &FakeReceiver) -> bool {
    receiver.state().samples().iter().any(|&sample| sample != 0)
}

/// Output `receiver`, as listed.
fn output(cx: &mut VisualTestContext, receiver: &FakeReceiver) -> AudioDevice {
    let name = receiver.device().name;
    let mut listed = None;
    wait(cx, "it to be listed", |cx| {
        listed = cx.update(|_, cx| {
            AudioEngine::devices(cx)
                .iter()
                .find(|device| device.name == name)
                .cloned()
        });
        listed.is_some()
    });
    let listed = listed.unwrap();
    AudioDevice {
        id: listed.id,
        name: listed.name,
    }
}

#[gpui::test]
fn a_speaker_found_is_an_output_and_plays_the_sound_once_picked(cx: &mut TestAppContext) {
    let speaker = FakeReceiver::start(Kind::Speaker);
    let (_, mut cx, _) = start(cx, &[&speaker], true);
    assert!(plays_through(&mut cx, HERE));
    let device = output(&mut cx, &speaker);
    assert!(device.id.starts_with("airplay:"), "{}", device.id);
    assert!(speaker.state().packets.is_empty(), "not till it's picked");

    cx.dispatch_action(output::set_device(Some(&device)));
    wait(&mut cx, "the tone to be heard through it", |cx| {
        plays_through(cx, "Fake Speaker") && heard(&speaker)
    });
    let shown = cx.update(|_, cx| AudioEngine::device_description(cx));
    assert_eq!(shown, "Fake Speaker · 44.1 kHz");

    cx.dispatch_action(output::set_device(None));
    wait(&mut cx, "the sound to come back here", |cx| {
        plays_through(cx, HERE)
    });
    assert!(speaker.wait_for(|state| state.closed > 0), "it's let go of");
}

#[gpui::test]
fn a_speaker_going_away_falls_back_as_settings_say_and_is_played_through_once_back(
    cx: &mut TestAppContext,
) {
    let speaker = FakeReceiver::start(Kind::Speaker);
    let (_, mut cx, speakers) = start(cx, &[&speaker], false);
    let device = output(&mut cx, &speaker);
    cx.dispatch_action(output::set_device(Some(&device)));
    wait(&mut cx, "it to play through it", |cx| {
        plays_through(cx, "Fake Speaker") && heard(&speaker)
    });

    // Settings say not to wait: this computer stands in for it.
    speakers.set_fakes(Vec::new());
    speaker.go_away();
    wait(&mut cx, "this computer to stand in", |cx| {
        playing(cx) == Some((HERE.into(), Some("Fake Speaker".into())))
    });

    let back = FakeReceiver::start(Kind::Speaker);
    speakers.set_fakes(vec![back.device()]);
    wait(&mut cx, "it to play through it once back", |cx| {
        plays_through(cx, "Fake Speaker") && heard(&back)
    });

    // Settings say to wait: nothing stands in for it.
    cx.update(|_, cx| {
        Settings::update(cx, false, |settings| {
            settings.audio_output.wait_for_device = true;
        });
    });
    speakers.set_fakes(Vec::new());
    back.go_away();
    wait(&mut cx, "the sound to wait for it", |cx| {
        cx.update(|_, cx| AudioEngine::route(cx).cloned())
            == Some(Route::Waiting("Fake Speaker".into()))
    });
}

/// Have the sheet up over `window` ask for the PIN `tv` shows, and give it.
fn pair(window: WindowHandle<Workspace>, cx: &mut VisualTestContext, tv: &FakeReceiver) {
    wait(cx, "a sheet to ask for the PIN", |cx| wants_pin(window, cx));
    assert!(tv.state().pin_shown, "the TV shows it");
    assert_eq!(sheet_says(window, cx), "Enter the PIN Fake TV shows.");
    assert!(tv.state().packets.is_empty(), "not till it's paired");
    cx.simulate_input(PIN);
    press(cx, "enter");
}

#[gpui::test]
fn a_tv_wants_its_pin_before_it_plays_the_apps_sound(cx: &mut TestAppContext) {
    let tv = FakeReceiver::start(Kind::Tv { pin: PIN });
    let (window, mut cx, _) = start(cx, &[&tv], false);
    let device = output(&mut cx, &tv);
    cx.dispatch_action(output::set_device(Some(&device)));
    pair(window, &mut cx, &tv);
    assert_eq!(
        playing(&mut cx),
        Some((HERE.into(), Some("Fake TV".into()))),
        "this computer stands in meanwhile"
    );

    wait(&mut cx, "it to play once paired", |cx| {
        plays_through(cx, "Fake TV") && heard(&tv)
    });
    assert!(!shows(&mut cx, "pairing"), "the sheet goes");
    let kept = cx.update(|_, cx| cx.global::<AppPairings>().0.paired());
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].1, "Fake TV");
}

#[gpui::test]
fn a_tv_a_block_plays_through_wants_its_pin_too(cx: &mut TestAppContext) {
    let tv = FakeReceiver::start(Kind::Tv { pin: PIN });
    let (window, mut cx, _) = start(cx, &[&tv], true);
    let device = output(&mut cx, &tv);
    cx.dispatch_action(output::set_block_device(Some(&device)));
    pair(window, &mut cx, &tv);
    wait(&mut cx, "the block to play through it once paired", |_| {
        heard(&tv)
    });
    assert!(plays_through(&mut cx, HERE), "the app's sound stays here");
}

/// The device the block in `window` plays through of its own, by name.
fn block_sound(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) -> Option<String> {
    window
        .read_with(cx, |workspace, cx| {
            let view = workspace.blocks[0].view.read(cx);
            view.sound_device().map(|device| device.name.clone())
        })
        .unwrap()
}

#[gpui::test]
fn a_tv_not_paired_with_is_picked_no_longer_and_the_sound_comes_back_here(cx: &mut TestAppContext) {
    let tv = FakeReceiver::start(Kind::Tv { pin: PIN });
    let speaker = FakeReceiver::start(Kind::Speaker);
    let (window, mut cx, _) = start(cx, &[&tv, &speaker], true);
    let device = output(&mut cx, &tv);
    cx.dispatch_action(output::set_device(Some(&device)));
    wait(&mut cx, "a sheet to ask for the PIN", |cx| {
        wants_pin(window, cx)
    });
    wait(&mut cx, "the sound to wait for it meanwhile", |cx| {
        cx.update(|_, cx| AudioEngine::route(cx).cloned()) == Some(Route::Waiting("Fake TV".into()))
    });
    press(&mut cx, "escape");
    wait(&mut cx, "the sound to come back here", |cx| {
        plays_through(cx, HERE)
    });
    let picked = cx.update(|_, cx| cx.global::<Settings>().audio_output.device.clone());
    assert_eq!(picked, None, "the system's default is picked again");

    // Picked for the block, by one that turns this computer away: once
    // the sheet saying so is closed, the block plays through the app's,
    // which plays on through what it picked.
    let other = output(&mut cx, &speaker);
    cx.dispatch_action(output::set_device(Some(&other)));
    wait(
        &mut cx,
        "the app's sound to play through the speaker",
        |cx| plays_through(cx, "Fake Speaker"),
    );
    tv.state().refuse_pairing = true;
    cx.dispatch_action(output::set_block_device(Some(&device)));
    wait(&mut cx, "the sheet to say why it can't pair", |cx| {
        sheet_says(window, cx).starts_with("Can't pair")
    });
    assert_eq!(block_sound(window, &mut cx), Some("Fake TV".into()));
    click(&mut cx, "pairing-cancel");
    wait(&mut cx, "the block to play through the app's", |cx| {
        block_sound(window, cx).is_none()
    });
    assert!(
        plays_through(&mut cx, "Fake Speaker"),
        "the app's own pick stays"
    );
    assert!(tv.state().packets.is_empty());
}

/// How many times `tv` was asked to show its PIN.
fn pins_shown(tv: &FakeReceiver) -> usize {
    tv.state().requests("POST", "/pair-pin-start").len()
}

#[gpui::test]
fn a_tv_asks_for_its_pin_once_and_cancelled_isnt_asked_again(cx: &mut TestAppContext) {
    let tv = FakeReceiver::start(Kind::Tv { pin: PIN });
    let (window, mut cx, _) = start(cx, &[&tv], false);
    let device = output(&mut cx, &tv);
    cx.dispatch_action(output::set_device(Some(&device)));
    wait(&mut cx, "a sheet to ask for the PIN", |cx| {
        wants_pin(window, cx)
    });
    let sheet = |cx: &mut VisualTestContext| {
        window
            .read_with(cx, |workspace, _| {
                workspace.pairing().map(|sheet| sheet.entity_id())
            })
            .unwrap()
    };
    let asking = sheet(&mut cx);
    assert_eq!(pins_shown(&tv), 1);

    // Routed again as it asks, to wait for the TV now: the sheet stays.
    cx.update(|_, cx| {
        Settings::update(cx, false, |settings| {
            settings.audio_output.wait_for_device = true;
        });
    });
    wait(&mut cx, "the sound to wait for it", |cx| {
        cx.update(|_, cx| AudioEngine::route(cx).cloned()) == Some(Route::Waiting("Fake TV".into()))
    });
    assert_eq!(sheet(&mut cx), asking, "the same sheet asks");
    assert_eq!(pins_shown(&tv), 1, "it shows the same PIN");

    press(&mut cx, "escape");
    wait(&mut cx, "the sheet to go", |cx| sheet(cx).is_none());
    assert!(
        !cx.update(|_, cx| AudioEngine::looks_for_device(cx)),
        "it isn't tried again, to ask again, till picked again"
    );
    assert!(tv.state().packets.is_empty());
    assert!(cx.update(|_, cx| cx.global::<AppPairings>().0.paired().is_empty()));
}

/// Forget the TV in Settings → Casting.
pub(super) fn forget_tv(cx: &mut VisualTestContext) {
    let mut settings = open_settings(cx);
    click(&mut settings, "pane-Casting");
    click(&mut settings, "forget-Fake TV");
    assert!(!shows(&mut settings, "forget-Fake TV"), "it goes");
}

#[gpui::test]
fn a_tv_forgotten_plays_the_apps_sound_no_longer(cx: &mut TestAppContext) {
    let tv = FakeReceiver::start(Kind::Tv { pin: PIN });
    let (window, mut cx, _) = start(cx, &[&tv], false);
    let device = output(&mut cx, &tv);
    cx.dispatch_action(output::set_device(Some(&device)));
    pair(window, &mut cx, &tv);
    wait(&mut cx, "it to play once paired", |cx| {
        plays_through(cx, "Fake TV") && heard(&tv)
    });

    forget_tv(&mut cx);
    wait(&mut cx, "the sound to come back here", |cx| {
        plays_through(cx, HERE)
    });
    let picked = cx.update(|_, cx| cx.global::<Settings>().audio_output.device.clone());
    assert_eq!(picked, None, "the system's default is picked again");
    assert!(tv.wait_for(|state| state.closed > 0), "it's let go of");
    assert!(!shows(&mut cx, "pairing"), "not asked for its PIN again");
}

#[gpui::test]
fn a_tv_forgotten_plays_a_blocks_sound_no_longer(cx: &mut TestAppContext) {
    let tv = FakeReceiver::start(Kind::Tv { pin: PIN });
    let (window, mut cx, _) = start(cx, &[&tv], true);
    let device = output(&mut cx, &tv);
    cx.dispatch_action(output::set_block_device(Some(&device)));
    pair(window, &mut cx, &tv);
    wait(&mut cx, "the block to play through it once paired", |_| {
        heard(&tv)
    });

    forget_tv(&mut cx);
    wait(&mut cx, "the block to play through the app's", |cx| {
        block_sound(window, cx).is_none()
    });
    assert!(tv.wait_for(|state| state.closed > 0), "it's let go of");
    assert!(plays_through(&mut cx, HERE));
}

/// Cast the block to `tv` from its menu, and wait for it to be told to
/// play.
fn cast_to(cx: &mut VisualTestContext, tv: &FakeReceiver) {
    let pairings = cx.update(|_, cx| Arc::clone(&cx.global::<AppPairings>().0));
    let target = AirPlayTarget::with(vec![tv.device()], pairings);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(target)])));
    click(cx, "cast");
    click(cx, "cast-0");
    wait(cx, "the TV to be told to play", |_| {
        !tv.state().requests("POST", "/play").is_empty()
    });
}

/// Play the block here again (`cast.stop`).
fn stop_cast(cx: &mut VisualTestContext) {
    TestAppContext::update(cx, |cx| api::run(Invocation::new("cast.stop"), cx)).unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn a_tv_cast_to_plays_the_apps_sound_again_once_the_cast_ends(cx: &mut TestAppContext) {
    let tv = FakeReceiver::start(Kind::Tv { pin: PIN });
    let (window, mut cx, speakers) = start(cx, &[&tv], true);
    let device = output(&mut cx, &tv);
    cx.dispatch_action(output::set_device(Some(&device)));
    pair(window, &mut cx, &tv);
    wait(&mut cx, "the app's sound to play through it", |_| {
        heard(&tv)
    });

    // It plays one thing at a time: the sound lets go of it, and waits
    // rather than take it back from the cast.
    let closed = tv.state().closed;
    cast_to(&mut cx, &tv);
    wait(&mut cx, "the sound to let go of it", |_| {
        tv.state().closed > closed
    });
    wait(&mut cx, "the sound to wait for it", |cx| {
        cx.update(|_, cx| AudioEngine::route(cx).cloned()) == Some(Route::Waiting("Fake TV".into()))
    });
    assert!(!speakers.is_there(&device.id), "not opened again meanwhile");

    let connections = tv.state().connections;
    stop_cast(&mut cx);
    wait(&mut cx, "the sound to play through it again", |cx| {
        plays_through(cx, "Fake TV") && tv.state().connections > connections
    });
}

#[gpui::test]
fn a_tv_a_block_plays_through_and_is_cast_to_plays_it_again_once_the_cast_ends(
    cx: &mut TestAppContext,
) {
    let tv = FakeReceiver::start(Kind::Tv { pin: PIN });
    let (window, mut cx, speakers) = start(cx, &[&tv], true);
    let device = output(&mut cx, &tv);
    cx.dispatch_action(output::set_block_device(Some(&device)));
    pair(window, &mut cx, &tv);
    wait(&mut cx, "the block to play through it", |_| heard(&tv));

    let closed = tv.state().closed;
    cast_to(&mut cx, &tv);
    wait(&mut cx, "the block's output to let go of it", |_| {
        tv.state().closed > closed
    });
    assert!(!speakers.is_there(&device.id), "not opened again meanwhile");
    assert_eq!(
        block_sound(window, &mut cx),
        Some("Fake TV".into()),
        "still picked"
    );

    let connections = tv.state().connections;
    stop_cast(&mut cx);
    wait(&mut cx, "its output to play through it again", |_| {
        tv.state().connections > connections
    });
}
