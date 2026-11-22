//! `output`: where the sound plays: the app's output, and a window's own.

use gpui::App;
use serde::Deserialize;
use serde_json::{Value, json};

use super::done;
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Args, Scope, Spec};
use crate::api::target;
use crate::api::{Error, Run};
use crate::app::{AudioEngine, Route, output_kind};
use crate::settings::{AudioDevice, Settings};

/// What picking no output of a window's own is called, wherever it's
/// picked: Audio → Window Output Device, a block's menu, the palette.
pub const SAME_AS_APP: &str = "Same as the App";

/// What the output commands take: an output, or none.
const DEVICE: &[Arg] = &[
    Arg::optional(
        "id",
        Kind::Text,
        "The output's id; none for the system's default, or for a window, the app's output.",
    ),
    Arg::optional(
        "name",
        Kind::Text,
        "What it's called, for an output not connected now, which is waited for.",
    ),
];

pub const COMMANDS: &[Spec] = &[
    Spec::query(
        "output.ls",
        "List Outputs",
        "List the outputs connected, the one picked, and where the sound goes now.",
        Scope::App,
        "{outputs: [{id, name, kind}], device: {id, name} or null, route}: the outputs \
         connected (kind AirPlay for a speaker on the network, else null), the one \
         picked (null for the system's default), and the route: {playing: {device, \
         rate, channels, instead_of}} (instead_of naming the one picked, when the \
         system's default stands in for it), {waiting: name}, {opening: name} or \
         {unavailable: why}.",
        |_, _, cx| Ok(listing(cx)),
    )
    .keywords(&[
        "list",
        "ls",
        "sound",
        "speakers",
        "headphones",
        "audio device",
    ]),
    Spec::app_with(
        "output.set_device",
        "Output Device",
        "Play the app's sound through an output, or the system's default. A Bluetooth \
         one paired and not connected is connected.",
        DEVICE,
        |args, cx| {
            let device = device(args, cx)?;
            if let Some(device) = &device {
                AudioEngine::bring(device, cx);
            }
            Settings::update(cx, true, |settings| settings.audio_output.device = device);
            done()
        },
    )
    .tells(&["route_changed", "settings_changed"])
    .keywords(&[
        "sound",
        "speakers",
        "headphones",
        "audio device",
        "bluetooth",
        "airplay",
    ])
    // The output picked, or null for the system's default.
    .state(|_, cx| Ok(json!(cx.global::<Settings>().audio_output.device))),
    Spec::window_with(
        "output.set_window_device",
        "Window Output Device",
        "Play the window's sound through an output of its own, or through the app's again.",
        DEVICE,
        |args, workspace, _, cx| {
            let device = device(args, cx)?;
            if let Some(device) = &device {
                AudioEngine::bring(device, cx);
            }
            workspace.set_sound_device(device, cx);
            done()
        },
    )
    .keywords(&[
        "sound",
        "speakers",
        "headphones",
        "audio device",
        "bluetooth",
        "airplay",
    ])
    // Its own output, or null for the app's.
    .state(|target, cx| {
        target::in_window(target, cx, |workspace, _, _| {
            Ok(json!(workspace.sound_device()))
        })
    }),
];

/// The outputs' events.
pub const EVENTS: &[EventKind] = &[
    EventKind::app(
        "outputs_changed",
        "Outputs were connected or disconnected.",
        &[EventField::new(
            "outputs",
            "Those connected now, as `output.ls` lists them.",
        )],
    )
    .causes(&["an output connected or disconnected"]),
    EventKind::app(
        "route_changed",
        "The app's sound goes elsewhere.",
        &[EventField::new(
            "route",
            "Where it goes now, as `output.ls` says.",
        )],
    )
    .causes(&["the system's own output changing"]),
];

/// What `output.ls` says: the outputs, the one picked, and where the
/// sound goes now.
pub fn listing(cx: &App) -> Value {
    let outputs: Vec<Value> = AudioEngine::devices(cx)
        .iter()
        .map(|device| {
            json!({
                "id": device.id,
                "name": device.name,
                "kind": output_kind(&device.id),
            })
        })
        .collect();
    json!({
        "outputs": outputs,
        "device": cx.global::<Settings>().audio_output.device,
        "route": route(AudioEngine::route(cx)),
    })
}

/// Where the sound goes, as `output.ls` says.
fn route(route: Option<&Route>) -> Value {
    match route {
        Some(Route::Device { playing, missing }) => json!({"playing": {
            "device": playing.name,
            "rate": playing.rate,
            "channels": playing.channels,
            "instead_of": missing,
        }}),
        Some(Route::Waiting(name)) => json!({"waiting": name}),
        Some(Route::Opening(name)) => json!({"opening": name}),
        Some(Route::Unavailable(why)) => json!({"unavailable": why}),
        None => json!({"unavailable": "there's no sound"}),
    }
}

#[derive(Deserialize)]
struct Pick {
    id: Option<String>,
    name: Option<String>,
}

/// The output `args` name: one listed, by id, or any by id and name.
fn device(args: Args, cx: &App) -> Result<Option<AudioDevice>, Error> {
    let Pick { id, name } = args.read()?;
    let Some(id) = id else {
        return match name {
            Some(_) => Err(Error::bad_args(args.command, "name goes with an id")),
            None => Ok(None),
        };
    };
    let name = match name {
        Some(name) => name,
        None => AudioEngine::devices(cx)
            .iter()
            .find(|device| device.id == id)
            .map(|device| device.name.clone())
            .ok_or_else(|| Error::not_found("output", id.clone()))?,
    };
    Ok(Some(AudioDevice { id, name }))
}

/// Play through `device`, or the system's default.
pub fn set_device(device: Option<&AudioDevice>) -> Run {
    Run::with("output.set_device", output(device))
}

/// Play the window in front through `device`, or the app's output again.
pub fn set_window_device(device: Option<&AudioDevice>) -> Run {
    Run::with("output.set_window_device", output(device))
}

/// The arguments the output commands take for `device`: by id and name,
/// as it may be one not connected now.
fn output(device: Option<&AudioDevice>) -> Value {
    device.map_or(
        Value::Null,
        |device| json!({"id": device.id, "name": device.name}),
    )
}

#[cfg(test)]
#[path = "tests/output.rs"]
mod tests;
