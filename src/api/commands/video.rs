//! `video`: a block's picture: its track, how it's turned, shaped and
//! cropped, and a screenshot of it; and every block's in a window, or in
//! every window.

use gpui::App;
use serde::Deserialize;
use serde_json::json;

use super::{done, select_track, select_tracks};
use crate::api::args::{Arg, Kind};
use crate::api::registry::{Args, Outcome, Spec, on_mac};
use crate::api::target;
use crate::api::{Error, Run};
use crate::media::tracks::TrackKind;
use crate::ui::framing::{ASPECT_RATIOS, CROPS, Ratio};
use crate::ui::player::PlayerView;
use crate::ui::workspace::all::Players;

pub const COMMANDS: &[Spec] = &[
    Spec::block(
        "block.video.next_track",
        "Next Video Track",
        "Show the file's next video track, the first after the last.",
        |player, _, cx| player.next_track(TrackKind::Video, cx),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["angle", "stream"])
    .setting("next_video_track")
    .keys(on_mac(&["ctrl-cmd-v"], &["ctrl-alt-v"])),
    Spec::block_with(
        "block.video.select_track",
        "Pick Video Track",
        "Show one of the file's video tracks.",
        TRACK,
        |args, player, _, cx| select_track(args, TrackKind::Video, player, cx),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["angle", "stream"]),
    Spec::block(
        "block.video.rotate_right",
        "Rotate Right",
        "Turn the picture a quarter turn clockwise.",
        |player, _, cx| player.rotate(1, cx),
    )
    .keywords(&["turn", "clockwise", "orientation"])
    .setting("rotate_right")
    .keys(&["secondary-r"]),
    Spec::block(
        "block.video.rotate_left",
        "Rotate Left",
        "Turn the picture a quarter turn counterclockwise.",
        |player, _, cx| player.rotate(-1, cx),
    )
    .keywords(&["turn", "counterclockwise", "orientation"])
    .setting("rotate_left")
    .keys(&["secondary-l"]),
    Spec::block(
        "block.video.flip_horizontally",
        "Flip Horizontally",
        "Mirror the picture left to right.",
        |player, _, cx| player.flip(true, cx),
    )
    .keywords(&["mirror"])
    .setting("flip_horizontally")
    .keys(&["secondary-shift-h"]),
    Spec::block(
        "block.video.flip_vertically",
        "Flip Vertically",
        "Turn the picture upside down, mirrored.",
        |player, _, cx| player.flip(false, cx),
    )
    .keywords(&["mirror", "upside down"])
    .setting("flip_vertically")
    .keys(&["secondary-shift-v"]),
    Spec::block(
        "block.video.next_aspect_ratio",
        "Next Aspect Ratio",
        "Show the picture at the next aspect ratio, then as the file is.",
        |player, _, cx| player.next_aspect_ratio(cx),
    )
    .keywords(&["shape", "stretch"])
    .setting("next_aspect_ratio")
    .keys(&["a"]),
    Spec::block_with(
        "block.video.set_aspect_ratio",
        "Aspect Ratio",
        "Show the picture at an aspect ratio, or as the file is.",
        ASPECT,
        |args, player, _, cx| {
            player.set_aspect_ratio(aspect(args)?, cx);
            done()
        },
    )
    .keywords(&["shape", "stretch"])
    .state(|target, cx| {
        target::read_block(target, cx, |player, _| {
            json!(player.adjustments().aspect.map(|ratio| ratio.name))
        })
    }),
    Spec::block(
        "block.video.next_crop",
        "Next Crop",
        "Crop the picture to the next shape, then not at all.",
        |player, _, cx| player.next_crop(cx),
    )
    .keywords(&["cut", "black bars", "zoom"])
    .setting("next_crop")
    .keys(&["c"]),
    Spec::block_with(
        "block.video.set_crop",
        "Crop",
        "Crop the picture to a shape, or not at all.",
        CROP,
        |args, player, _, cx| {
            player.set_crop(crop(args)?, cx);
            done()
        },
    )
    .keywords(&["cut", "black bars", "zoom"])
    .state(|target, cx| {
        target::read_block(target, cx, |player, _| {
            json!(player.adjustments().crop.map(|ratio| ratio.name))
        })
    }),
    Spec::block(
        "block.video.reset_picture",
        "Reset Picture",
        "Show the picture as the file is: not turned, flipped, reshaped or cropped.",
        |player, _, cx| player.reset_picture(cx),
    )
    .keywords(&["undo", "original"])
    .setting("reset_picture")
    .keys(&["secondary-0"]),
    Spec::block(
        "block.video.take_screenshot",
        "Take Screenshot",
        "Save the frame showing as a file, and say where it went.",
        |player, window, cx| player.take_screenshot(window, cx),
    )
    .keywords(&["capture", "image", "frame", "snapshot"])
    .setting("take_screenshot")
    .keys(&["s"]),
    Spec::blocks(
        "window.video.next_track",
        "Next Video Track in Window",
        "Show the next video track of every file in the window, the first after the last.",
        |players, cx| players.each(cx, |player, cx| player.next_track(TrackKind::Video, cx)),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["every", "all", "angle", "stream"]),
    Spec::blocks_with(
        "window.video.select_track",
        "Pick Video Track in Window",
        "Show a video track of every file in the window that has it.",
        TRACK,
        |args, players, cx| select_tracks(args, TrackKind::Video, players, cx),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["every", "all", "angle", "stream"]),
    Spec::blocks(
        "window.video.rotate_right",
        "Rotate Right in Window",
        "Turn the picture of every file in the window a quarter turn clockwise.",
        |players, cx| players.each(cx, |player, cx| player.rotate(1, cx)),
    )
    .keywords(&["every", "all", "turn", "clockwise", "orientation"]),
    Spec::blocks(
        "window.video.rotate_left",
        "Rotate Left in Window",
        "Turn the picture of every file in the window a quarter turn counterclockwise.",
        |players, cx| players.each(cx, |player, cx| player.rotate(-1, cx)),
    )
    .keywords(&["every", "all", "turn", "counterclockwise", "orientation"]),
    Spec::blocks(
        "window.video.flip_horizontally",
        "Flip Horizontally in Window",
        "Mirror the picture of every file in the window left to right.",
        |players, cx| players.each(cx, |player, cx| player.flip(true, cx)),
    )
    .keywords(&["every", "all", "mirror"]),
    Spec::blocks(
        "window.video.flip_vertically",
        "Flip Vertically in Window",
        "Turn the picture of every file in the window upside down, mirrored.",
        |players, cx| players.each(cx, |player, cx| player.flip(false, cx)),
    )
    .keywords(&["every", "all", "mirror", "upside down"]),
    Spec::blocks(
        "window.video.next_aspect_ratio",
        "Next Aspect Ratio in Window",
        "Show the picture of every file in the window at its next aspect ratio, then as the file is.",
        |players, cx| players.each(cx, PlayerView::next_aspect_ratio),
    )
    .keywords(&["every", "all", "shape", "stretch"]),
    Spec::blocks_with(
        "window.video.set_aspect_ratio",
        "Aspect Ratio in Window",
        "Show the picture of every file in the window at an aspect ratio, or as the file is.",
        ASPECT,
        set_aspect_ratio_of_each,
    )
    .keywords(&["every", "all", "shape", "stretch"]),
    Spec::blocks(
        "window.video.next_crop",
        "Next Crop in Window",
        "Crop the picture of every file in the window to its next shape, then not at all.",
        |players, cx| players.each(cx, PlayerView::next_crop),
    )
    .keywords(&["every", "all", "cut", "black bars", "zoom"]),
    Spec::blocks_with(
        "window.video.set_crop",
        "Crop in Window",
        "Crop the picture of every file in the window to a shape, or not at all.",
        CROP,
        set_crop_of_each,
    )
    .keywords(&["every", "all", "cut", "black bars", "zoom"]),
    Spec::blocks(
        "window.video.reset_picture",
        "Reset Picture in Window",
        "Show the picture of every file in the window as the file is: not turned, flipped, reshaped or cropped.",
        |players, cx| players.each(cx, PlayerView::reset_picture),
    )
    .keywords(&["every", "all", "undo", "original"]),
    Spec::blocks(
        "window.video.take_screenshot",
        "Take Screenshot in Window",
        "Save the frame showing in every file in the window as a file, and say where each went.",
        |players, cx| players.each_in_window(cx, PlayerView::take_screenshot),
    )
    .keywords(&["every", "all", "capture", "image", "frame", "snapshot"]),
    Spec::every_block(
        "app.video.next_track",
        "Next Video Track in Every Window",
        "Show the next video track of every file in every window, the first after the last.",
        |players, cx| players.each(cx, |player, cx| player.next_track(TrackKind::Video, cx)),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["every", "all", "angle", "stream"]),
    Spec::every_block_with(
        "app.video.select_track",
        "Pick Video Track in Every Window",
        "Show a video track of every file in every window that has it.",
        TRACK,
        |args, players, cx| select_tracks(args, TrackKind::Video, players, cx),
    )
    .tells(&["block.playback.track_changed"])
    .keywords(&["every", "all", "angle", "stream"]),
    Spec::every_block(
        "app.video.rotate_right",
        "Rotate Right in Every Window",
        "Turn the picture of every file in every window a quarter turn clockwise.",
        |players, cx| players.each(cx, |player, cx| player.rotate(1, cx)),
    )
    .keywords(&["every", "all", "turn", "clockwise", "orientation"]),
    Spec::every_block(
        "app.video.rotate_left",
        "Rotate Left in Every Window",
        "Turn the picture of every file in every window a quarter turn counterclockwise.",
        |players, cx| players.each(cx, |player, cx| player.rotate(-1, cx)),
    )
    .keywords(&["every", "all", "turn", "counterclockwise", "orientation"]),
    Spec::every_block(
        "app.video.flip_horizontally",
        "Flip Horizontally in Every Window",
        "Mirror the picture of every file in every window left to right.",
        |players, cx| players.each(cx, |player, cx| player.flip(true, cx)),
    )
    .keywords(&["every", "all", "mirror"]),
    Spec::every_block(
        "app.video.flip_vertically",
        "Flip Vertically in Every Window",
        "Turn the picture of every file in every window upside down, mirrored.",
        |players, cx| players.each(cx, |player, cx| player.flip(false, cx)),
    )
    .keywords(&["every", "all", "mirror", "upside down"]),
    Spec::every_block(
        "app.video.next_aspect_ratio",
        "Next Aspect Ratio in Every Window",
        "Show the picture of every file in every window at its next aspect ratio, then as the file is.",
        |players, cx| players.each(cx, PlayerView::next_aspect_ratio),
    )
    .keywords(&["every", "all", "shape", "stretch"]),
    Spec::every_block_with(
        "app.video.set_aspect_ratio",
        "Aspect Ratio in Every Window",
        "Show the picture of every file in every window at an aspect ratio, or as the file is.",
        ASPECT,
        set_aspect_ratio_of_each,
    )
    .keywords(&["every", "all", "shape", "stretch"]),
    Spec::every_block(
        "app.video.next_crop",
        "Next Crop in Every Window",
        "Crop the picture of every file in every window to its next shape, then not at all.",
        |players, cx| players.each(cx, PlayerView::next_crop),
    )
    .keywords(&["every", "all", "cut", "black bars", "zoom"]),
    Spec::every_block_with(
        "app.video.set_crop",
        "Crop in Every Window",
        "Crop the picture of every file in every window to a shape, or not at all.",
        CROP,
        set_crop_of_each,
    )
    .keywords(&["every", "all", "cut", "black bars", "zoom"]),
    Spec::every_block(
        "app.video.reset_picture",
        "Reset Picture in Every Window",
        "Show the picture of every file in every window as the file is: not turned, flipped, reshaped or cropped.",
        |players, cx| players.each(cx, PlayerView::reset_picture),
    )
    .keywords(&["every", "all", "undo", "original"]),
    Spec::every_block(
        "app.video.take_screenshot",
        "Take Screenshot in Every Window",
        "Save the frame showing in every file in every window as a file, and say where each went.",
        |players, cx| players.each_in_window(cx, PlayerView::take_screenshot),
    )
    .keywords(&["every", "all", "capture", "image", "frame", "snapshot"]),
];

/// A video track to show.
const TRACK: &[Arg] = &[Arg::required(
    "number",
    Kind::Integer { min: 1 },
    "Which, from 1, as the tracks menu lists them.",
)];

/// An aspect ratio to show the picture at.
const ASPECT: &[Arg] = &[Arg::optional(
    "ratio",
    Kind::Choice(|| names(&ASPECT_RATIOS)),
    "The aspect ratio; none for the file's own.",
)];

/// A shape to crop the picture to.
const CROP: &[Arg] = &[Arg::optional(
    "crop",
    Kind::Choice(|| names(&CROPS)),
    "The shape; none for no crop.",
)];

#[derive(Deserialize)]
struct Aspect {
    ratio: Option<String>,
}

#[derive(Deserialize)]
struct Crop {
    crop: Option<String>,
}

/// The aspect ratio `args` name; none for the file's own.
fn aspect(args: Args) -> Result<Option<Ratio>, Error> {
    let Aspect { ratio } = args.read()?;
    Ok(named(&ASPECT_RATIOS, ratio))
}

/// The crop `args` name; none for no crop.
fn crop(args: Args) -> Result<Option<Ratio>, Error> {
    let Crop { crop } = args.read()?;
    Ok(named(&CROPS, crop))
}

/// Show each at the aspect ratio `args` name.
fn set_aspect_ratio_of_each(args: Args, players: &Players, cx: &mut App) -> Outcome {
    let ratio = aspect(args)?;
    players.each(cx, |player, cx| player.set_aspect_ratio(ratio, cx));
    done()
}

/// Crop each as `args` say.
fn set_crop_of_each(args: Args, players: &Players, cx: &mut App) -> Outcome {
    let crop = crop(args)?;
    players.each(cx, |player, cx| player.set_crop(crop, cx));
    done()
}

fn names(ratios: &[Ratio]) -> Vec<&'static str> {
    ratios.iter().map(|ratio| ratio.name).collect()
}

/// The one of `ratios` called `name`, checked already to be one.
fn named(ratios: &[Ratio], name: Option<String>) -> Option<Ratio> {
    let name = name?;
    ratios.iter().copied().find(|ratio| ratio.name == name)
}

/// Show the block in front at `ratio`, or as the file is.
pub fn set_aspect_ratio(ratio: Option<Ratio>) -> Run {
    Run::with(
        "block.video.set_aspect_ratio",
        json!({"ratio": ratio.map(|ratio| ratio.name)}),
    )
}

/// Crop the block in front to `crop`, or not at all.
pub fn set_crop(crop: Option<Ratio>) -> Run {
    Run::with(
        "block.video.set_crop",
        json!({"crop": crop.map(|crop| crop.name)}),
    )
}
