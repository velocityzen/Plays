//! `video`: a tile's picture: its track, how it's turned, shaped and
//! cropped, and a screenshot of it.

use serde::Deserialize;
use serde_json::json;

use super::done;
use crate::api::args::{Arg, Kind};
use crate::api::registry::Spec;
use crate::api::target;
use crate::hotkeys::Command;
use crate::media::tracks::TrackKind;
use crate::ui::framing::{ASPECT_RATIOS, CROPS, Ratio};

pub const COMMANDS: &[Spec] = &[
    Spec::tile(
        "video.next_track",
        "Next Video Track",
        "Show the file's next video track, the first after the last.",
        |player, _, cx| player.next_track(TrackKind::Video, cx),
    )
    .keywords(&["angle", "stream"])
    .hotkey(Command::NextVideoTrack),
    Spec::tile(
        "video.rotate_right",
        "Rotate Right",
        "Turn the picture a quarter turn clockwise.",
        |player, _, cx| player.rotate(1, cx),
    )
    .keywords(&["turn", "clockwise", "orientation"])
    .hotkey(Command::RotateRight),
    Spec::tile(
        "video.rotate_left",
        "Rotate Left",
        "Turn the picture a quarter turn counterclockwise.",
        |player, _, cx| player.rotate(-1, cx),
    )
    .keywords(&["turn", "counterclockwise", "orientation"])
    .hotkey(Command::RotateLeft),
    Spec::tile(
        "video.flip_horizontally",
        "Flip Horizontally",
        "Mirror the picture left to right.",
        |player, _, cx| player.flip(true, cx),
    )
    .keywords(&["mirror"])
    .hotkey(Command::FlipHorizontally),
    Spec::tile(
        "video.flip_vertically",
        "Flip Vertically",
        "Turn the picture upside down, mirrored.",
        |player, _, cx| player.flip(false, cx),
    )
    .keywords(&["mirror", "upside down"])
    .hotkey(Command::FlipVertically),
    Spec::tile(
        "video.next_aspect_ratio",
        "Next Aspect Ratio",
        "Show the picture at the next aspect ratio, then as the file is.",
        |player, _, cx| player.next_aspect_ratio(cx),
    )
    .keywords(&["shape", "stretch"])
    .hotkey(Command::NextAspectRatio),
    Spec::tile_with(
        "video.set_aspect_ratio",
        "Aspect Ratio",
        "Show the picture at an aspect ratio, or as the file is.",
        &[Arg::optional(
            "ratio",
            Kind::Choice(|| names(&ASPECT_RATIOS)),
            "The aspect ratio; none for the file's own.",
        )],
        |args, player, _, cx| {
            let Aspect { ratio } = args.read()?;
            player.set_aspect_ratio(named(&ASPECT_RATIOS, ratio), cx);
            done()
        },
    )
    .keywords(&["shape", "stretch"])
    .state(|target, cx| {
        target::read_tile(target, cx, |player, _| {
            json!(player.adjustments().aspect.map(|ratio| ratio.name))
        })
    }),
    Spec::tile(
        "video.next_crop",
        "Next Crop",
        "Crop the picture to the next shape, then not at all.",
        |player, _, cx| player.next_crop(cx),
    )
    .keywords(&["cut", "black bars", "zoom"])
    .hotkey(Command::NextCrop),
    Spec::tile_with(
        "video.set_crop",
        "Crop",
        "Crop the picture to a shape, or not at all.",
        &[Arg::optional(
            "crop",
            Kind::Choice(|| names(&CROPS)),
            "The shape; none for no crop.",
        )],
        |args, player, _, cx| {
            let Crop { crop } = args.read()?;
            player.set_crop(named(&CROPS, crop), cx);
            done()
        },
    )
    .keywords(&["cut", "black bars", "zoom"])
    .state(|target, cx| {
        target::read_tile(target, cx, |player, _| {
            json!(player.adjustments().crop.map(|ratio| ratio.name))
        })
    }),
    Spec::tile(
        "video.reset_picture",
        "Reset Picture",
        "Show the picture as the file is: not turned, flipped, reshaped or cropped.",
        |player, _, cx| player.reset_picture(cx),
    )
    .keywords(&["undo", "original"])
    .hotkey(Command::ResetPicture),
    Spec::tile(
        "video.take_screenshot",
        "Take Screenshot",
        "Save the frame showing as a file, and say where it went.",
        |player, window, cx| player.take_screenshot(window, cx),
    )
    .keywords(&["capture", "image", "frame", "snapshot"])
    .hotkey(Command::TakeScreenshot),
];

#[derive(Deserialize)]
struct Aspect {
    ratio: Option<String>,
}

#[derive(Deserialize)]
struct Crop {
    crop: Option<String>,
}

fn names(ratios: &[Ratio]) -> Vec<&'static str> {
    ratios.iter().map(|ratio| ratio.name).collect()
}

/// The one of `ratios` called `name`, checked already to be one.
fn named(ratios: &[Ratio], name: Option<String>) -> Option<Ratio> {
    let name = name?;
    ratios.iter().copied().find(|ratio| ratio.name == name)
}
