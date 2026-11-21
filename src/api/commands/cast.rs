//! `cast`: playing a tile on a TV or speaker found on the network.

use serde::Deserialize;
use serde_json::json;

use super::done;
use crate::api::Error;
use crate::api::args::{Arg, Kind};
use crate::api::registry::Spec;
use crate::api::target;
use crate::cast::{Casting, Device};

pub const COMMANDS: &[Spec] = &[
    Spec::tile_with(
        "cast.to",
        "Play On",
        "Play the tile on a TV or speaker found on the network, from where it is.",
        &[
            Arg::required("device", Kind::Text, "The device's id."),
            Arg::optional(
                "kind",
                Kind::Text,
                "How it's reached, such as Chromecast, AirPlay or DLNA, for an id two share.",
            ),
        ],
        |args, player, _, cx| {
            let To { device, kind } = args.read()?;
            let is = |found: &&Device| {
                found.id == device
                    && kind
                        .as_deref()
                        .is_none_or(|kind| kind.eq_ignore_ascii_case(found.kind))
            };
            // What it casts to stays one while it isn't found.
            let found = Casting::devices(cx)
                .iter()
                .chain(player.casting())
                .find(is)
                .cloned()
                .ok_or_else(|| Error::not_found("device", device.clone()))?;
            player.cast_to(Some(found), cx);
            done()
        },
    )
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
    .keywords(&["cast", "stop casting", "disconnect", "this computer"]),
];

#[derive(Deserialize)]
struct To {
    device: String,
    kind: Option<String>,
}
