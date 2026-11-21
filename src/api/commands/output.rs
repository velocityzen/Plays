//! `output`: where the sound plays: the app's output, and a window's own.

use gpui::App;
use serde::Deserialize;
use serde_json::json;

use super::done;
use crate::api::Error;
use crate::api::args::{Arg, Kind};
use crate::api::registry::{Args, Spec};
use crate::api::target;
use crate::app::AudioEngine;
use crate::settings::{AudioDevice, Settings};

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
