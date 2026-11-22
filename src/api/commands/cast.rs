//! `cast`: playing a tile on a TV or speaker found on the network.

use std::collections::BTreeMap;

use gpui::App;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::done;
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Args, Outcome, Scope, Spec};
use crate::api::target::{self, Target};
use crate::api::{Error, Run};
use crate::cast::{Casting, Device, Shows};

/// Which device: its id, and how it's reached for an id two share.
const DEVICE: &[Arg] = &[
    Arg::required("device", Kind::Text, "The device's id."),
    Arg::optional(
        "kind",
        Kind::Text,
        "How it's reached, such as Chromecast, AirPlay or DLNA, for an id two share.",
    ),
];

pub const COMMANDS: &[Spec] = &[
    Spec::query(
        "cast.ls",
        "List Devices",
        "List the TVs and speakers found on the network to play on, by how they're \
         reached.",
        Scope::App,
        "{Chromecast: [{id, name, shows}], AirPlay: […], DLNA: […]}: each protocol's \
         devices found, by name, shows being video for a TV and sound for a speaker; a \
         protocol with none found isn't there.",
        |_, _, cx| Ok(found(cx)),
    )
    .keywords(&[
        "list",
        "ls",
        "cast",
        "tv",
        "chromecast",
        "airplay",
        "dlna",
        "speaker",
    ]),
    Spec::tile_with(
        "cast.to",
        "Play On",
        "Play the tile on a TV or speaker found on the network, from where it is.",
        DEVICE,
        |args, player, _, cx| {
            let device = find(args, player.casting(), cx)?;
            player.cast_to(Some(device), cx);
            done()
        },
    )
    .tells(&["cast_started"])
    .keywords(&[
        "cast",
        "tv",
        "chromecast",
        "airplay",
        "dlna",
        "speaker",
        "stream",
    ])
    // The device it casts to, if any.
    .state(|target, cx| {
        target::read_tile(target, cx, |player, _| {
            json!(player.casting().map(|device| json!({
                "id": device.id,
                "kind": device.kind,
                "name": device.name,
            })))
        })
    }),
    Spec::tile(
        "cast.stop",
        "Play Here",
        "Stop casting the tile, and play it here from where the device got to.",
        |player, _, cx| player.cast_to(None, cx),
    )
    .tells(&["cast_ended"])
    .keywords(&["cast", "stop casting", "disconnect", "this computer"]),
    Spec::custom_with(
        "cast.pair",
        "Pair and Play On",
        "Pair with a TV that shows a PIN to type, such as an Apple TV, then play the \
         tile on it.",
        Scope::Tile,
        DEVICE,
        pair_with,
    )
    .keywords(&["cast", "pin", "code", "apple tv", "airplay"]),
];

/// Casting's events.
pub const EVENTS: &[EventKind] = &[
    EventKind::app(
        "cast_devices_changed",
        "TVs or speakers to cast to were found on the network, or went.",
        &[EventField::new(
            "devices",
            "Those found now, as `cast.ls` lists them.",
        )],
    )
    .causes(&["devices found on the network or gone from it"]),
    EventKind::tile(
        "cast_started",
        "A tile began playing on a TV or speaker.",
        &[EventField::new(
            "device",
            "The device: its id, kind and name.",
        )],
    )
    .causes(&["a TV paired with `cast.pair` once its PIN is typed"]),
    EventKind::tile(
        "cast_ended",
        "A tile stopped playing on a TV or speaker.",
        &[EventField::new(
            "device",
            "The device it played on: its id, kind and name.",
        )],
    )
    .causes(&["the device stopping or leaving the network"]),
];

/// The devices found, by protocol, each protocol's by name.
pub fn found(cx: &App) -> Value {
    // `Casting` keeps them by name, so each protocol's are too.
    let mut found: BTreeMap<&str, Vec<&Device>> = BTreeMap::new();
    for device in Casting::devices(cx) {
        found.entry(device.kind).or_default().push(device);
    }
    let found: Map<String, Value> = found
        .into_iter()
        .map(|(kind, devices)| {
            let devices: Vec<Value> = devices
                .into_iter()
                .map(|device| {
                    let shows = match device.shows {
                        Shows::Video => "video",
                        Shows::Sound => "sound",
                    };
                    json!({"id": device.id, "name": device.name, "shows": shows})
                })
                .collect();
            (kind.to_owned(), Value::from(devices))
        })
        .collect();
    Value::Object(found)
}

#[derive(Deserialize)]
struct To {
    device: String,
    kind: Option<String>,
}

/// The device `args` name, among those found and the one the tile casts
/// to, `casting`, which stays one while it isn't found.
fn find(args: Args, casting: Option<&Device>, cx: &App) -> Result<Device, Error> {
    let To { device, kind } = args.read()?;
    let is = |found: &&Device| {
        found.id == device
            && kind
                .as_deref()
                .is_none_or(|kind| kind.eq_ignore_ascii_case(found.kind))
    };
    Casting::devices(cx)
        .iter()
        .chain(casting)
        .find(is)
        .cloned()
        .ok_or_else(|| Error::not_found("device", device.clone()))
}

/// Ask for the PIN the device shows, over the tile's window, then cast the
/// tile there.
fn pair_with(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, tile) = target::tile(target, cx)?;
    let device = find(args, tile.read(cx).casting(), cx)?;
    window
        .update(cx, |workspace, _, cx| workspace.pair(&tile, device, cx))
        .map_err(|_| Error::failed("the window has closed"))?;
    done()
}

/// Cast the tile in front to `device`, or play it here again.
pub fn to(device: Option<&Device>) -> Run {
    match device {
        Some(device) => Run::with("cast.to", json!({"device": device.id, "kind": device.kind})),
        None => Run::command("cast.stop"),
    }
}

/// Pair with `device`, then cast the tile in front to it.
pub fn pair(device: &Device) -> Run {
    Run::with(
        "cast.pair",
        json!({"device": device.id, "kind": device.kind}),
    )
}
