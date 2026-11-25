//! Bluetooth headphones paired with this computer, from fakes of macOS's
//! Bluetooth and outputs, picked as someone at the screen would: listed
//! while they aren't connected, connected once picked, in Settings or for
//! a block, even picked already, and played through once they are; not
//! connected for starting the app, nor once they go away.

use std::sync::Mutex;
use std::time::Duration;

use gpui::{TestAppContext, VisualTestContext};

use serde_json::json;

use super::casting::wait;
use crate::api::commands::output;
use crate::api::{self, Invocation};
use crate::app::{AudioEngine, Bluetooth, Devices, Local, Paired, Route};
use crate::media::AudioOutput;
use crate::media::output::{DeviceEvent, DeviceInfo, OpenError, Playing};
use crate::settings::{AudioDevice, Settings};
use crate::ui::testing::{self, click, open_settings};

const HERE: &str = "This computer";
/// Paired, and connected from the start.
const PODS: (&str, &str) = ("00-00-5e-00-53-2b", "Pods");
/// Paired, and not connected.
const RADIO: (&str, &str) = ("00-00-5e-00-53-2a", "Radio");
/// Long enough for a device picked to have been connected, were it to be.
const SETTLE: Duration = Duration::from_millis(300);

/// The outputs connected besides this computer's, by id.
static CONNECTED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The addresses of the devices asked to connect.
static ASKED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Tells an output playing through a device what happens to it.
type OnEvent = Box<dyn Fn(DeviceEvent) + Send>;
/// What tells the outputs playing through a device what happens to it.
static ON_EVENTS: Mutex<Vec<OnEvent>> = Mutex::new(Vec::new());

/// The id macOS gives the output of the device at `address` once it's
/// connected.
fn output_id(address: &str) -> String {
    format!("coreaudio:{}:output", address.to_ascii_uppercase())
}

fn name_of(id: &str) -> &'static str {
    [PODS, RADIO]
        .into_iter()
        .find(|(address, _)| output_id(address) == id)
        .map_or(HERE, |(_, name)| name)
}

fn connected() -> Vec<DeviceInfo> {
    let mut all = vec![DeviceInfo {
        id: "coreaudio:Speakers".into(),
        name: HERE.into(),
        bluetooth: false,
    }];
    all.extend(CONNECTED.lock().unwrap().iter().map(|id| DeviceInfo {
        id: id.clone(),
        name: name_of(id).into(),
        bluetooth: true,
    }));
    all
}

fn is_connected(id: Option<&str>) -> bool {
    id.is_none_or(|id| connected().iter().any(|device| device.id == id))
}

fn open(
    output: &mut AudioOutput,
    id: Option<&str>,
    on_event: OnEvent,
) -> Result<Playing, OpenError> {
    if !is_connected(id) {
        return Err(OpenError::Missing);
    }
    output.play_nowhere();
    ON_EVENTS.lock().unwrap().push(on_event);
    Ok(Playing {
        id: id.map(str::to_owned),
        name: id.map_or(HERE, name_of).into(),
        rate: 48_000,
        channels: 2,
    })
}

fn paired() -> Vec<Paired> {
    [PODS, RADIO]
        .into_iter()
        .map(|(address, name)| Paired {
            address: address.into(),
            name: name.into(),
        })
        .collect()
}

/// As macOS connects one: its output comes up.
fn connect(address: &str) -> Result<(), String> {
    ASKED.lock().unwrap().push(address.to_owned());
    let id = output_id(address);
    let mut connected = CONNECTED.lock().unwrap();
    if !connected.contains(&id) {
        connected.push(id);
    }
    Ok(())
}

/// The device of output `id` goes away, as headphones turned off do.
fn lose(id: &str) {
    CONNECTED
        .lock()
        .unwrap()
        .retain(|connected| connected != id);
    let on_events = std::mem::take(&mut *ON_EVENTS.lock().unwrap());
    for on_event in on_events {
        on_event(DeviceEvent::Lost);
    }
}

fn asked() -> Vec<String> {
    ASKED.lock().unwrap().clone()
}

fn route(cx: &mut VisualTestContext) -> Option<Route> {
    cx.update(|_, cx| AudioEngine::route(cx).cloned())
}

/// Whether the app's sound goes through `name`, picked.
fn plays_through(cx: &mut VisualTestContext, name: &str) -> bool {
    matches!(route(cx), Some(Route::Device { playing, missing: None }) if playing.name == name)
}

/// The outputs listed, by name.
fn listed(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        AudioEngine::devices(cx)
            .iter()
            .map(|device| device.name.clone())
            .collect()
    })
}

fn device((address, name): (&str, &str)) -> AudioDevice {
    AudioDevice {
        id: output_id(address),
        name: name.into(),
    }
}

#[gpui::test]
fn headphones_paired_are_outputs_and_picking_them_connects_them(cx: &mut TestAppContext) {
    CONNECTED.lock().unwrap().push(output_id(PODS.0));
    let (window, mut cx) = testing::start(cx);
    let radio = device(RADIO);
    cx.update(|_, cx| {
        // Picked last time, and waited for.
        Settings::update(cx, false, |settings| {
            settings.audio_output.device = Some(radio.clone());
            settings.audio_output.wait_for_device = true;
        });
        let devices = Devices {
            local: Local {
                list: connected,
                is_connected,
                open,
                watch: drop,
            },
            bluetooth: Bluetooth { paired, connect },
            speakers: None,
        };
        AudioEngine::start_with(devices, cx);
    });
    wait(&mut cx, "the headphones to be listed", |cx| {
        listed(cx).contains(&RADIO.1.to_owned())
    });
    assert_eq!(listed(&mut cx), [HERE, PODS.1, RADIO.1], "each once");
    wait(&mut cx, "the sound to wait for the radio", |cx| {
        route(cx) == Some(Route::Waiting(RADIO.1.into()))
    });
    std::thread::sleep(SETTLE);
    assert!(asked().is_empty(), "not for starting the app");

    // Picked again, in Settings.
    let mut settings = open_settings(&mut cx);
    click(&mut settings, "pane-Audio");
    click(&mut settings, "audio-device");
    // Under System Default, this computer's and Pods.
    click(&mut settings, "3");
    wait(
        &mut cx,
        "the sound to play through them once connected",
        |cx| plays_through(cx, RADIO.1),
    );
    assert_eq!(asked(), [RADIO.0]);
    cx.update(|_, cx| AudioEngine::list_devices(cx));
    cx.run_until_parked();
    assert_eq!(
        listed(&mut cx),
        [HERE, PODS.1, RADIO.1],
        "once connected, still once"
    );

    // Gone: waited for, not connected again.
    lose(&radio.id);
    wait(&mut cx, "the sound to wait for them", |cx| {
        route(cx) == Some(Route::Waiting(RADIO.1.into()))
    });
    std::thread::sleep(SETTLE);
    assert_eq!(asked(), [RADIO.0], "not once they go away");

    // A block picks one connected already, and one not paired.
    super::open(window, &mut cx, &["tone.m4a"], false);
    cx.dispatch_action(output::set_block_device(Some(&device(PODS))));
    cx.dispatch_action(output::set_block_device(Some(&device((
        "00-00-5e-00-53-2c",
        "Stranger",
    )))));
    cx.run_until_parked();
    std::thread::sleep(SETTLE);
    assert_eq!(asked(), [RADIO.0], "nothing to connect");

    // And them: the app's sound, waiting for them, plays through them too.
    cx.dispatch_action(output::set_block_device(Some(&radio)));
    wait(&mut cx, "the sound to play through them again", |cx| {
        plays_through(cx, RADIO.1)
    });
    assert_eq!(asked(), [RADIO.0, RADIO.0]);

    // By its id alone, as from the command line: named as it's listed.
    let pods = device(PODS);
    let by_id = Invocation::new("output.set_device").with_args(json!({"id": pods.id}));
    TestAppContext::update(&cx, |cx| api::run(by_id, cx)).unwrap();
    let picked = cx.update(|_, cx| cx.global::<Settings>().audio_output.device.clone());
    assert_eq!(picked, Some(pods));
}
