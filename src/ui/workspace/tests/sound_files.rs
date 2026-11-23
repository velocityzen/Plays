//! A sound file's block: its cover, sized to the block, and under it its
//! title, who made it, its album and its quality.

use std::path::PathBuf;

use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, VisualizerShader, px, size, white};

use super::{drawn_in, open};
use crate::files::test_folder;
use crate::settings::{Settings, Visualizer};
use crate::ui::testing::{self, find, fixture, shows, wait_until};
use crate::ui::theme::player::{
    COVER_MIN_SIZE, NO_COVER_ICON_SCALE, NO_COVER_SCALE, NOTE_TOP, SPEAKER_OPACITY,
};

#[gpui::test]
fn a_tagged_song_shows_its_title_artist_album_and_quality(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["tune.flac"], false);
    for (selector, line) in [
        ("sound-title", "Tone"),
        ("sound-artist", "Plays"),
        ("sound-album", "Fixtures — Various Artists"),
        ("sound-quality", "FLAC · 24-bit · 96 kHz · Mono · 489 kb/s"),
    ] {
        assert_eq!(drawn_in(&mut cx, selector), [line], "{selector}");
    }
}

#[gpui::test]
fn a_sound_with_no_tags_is_titled_by_its_file(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["tone.m4a"], false);
    assert_eq!(drawn_in(&mut cx, "sound-title"), ["tone.m4a"]);
    assert!(!shows(&mut cx, "sound-artist"));
    assert!(!shows(&mut cx, "sound-album"));
    assert_eq!(
        drawn_in(&mut cx, "sound-quality"),
        ["AAC LC · 48 kHz · Mono · 32 kb/s"]
    );
}

/// The block's padding at its sides.
const SIDES: Pixels = px(16.);

/// A copy of tone.m4a in a folder of its own, with a small cover.png,
/// wider than tall, beside it.
fn sound_with_a_cover() -> PathBuf {
    let folder = test_folder("sound-cover");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("tone.m4a");
    std::fs::copy(fixture("tone.m4a"), &path).unwrap();
    let file = std::fs::File::create(folder.join("cover.png")).unwrap();
    let mut encoder = png::Encoder::new(file, 48, 32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&[200; 48 * 32 * 3]).unwrap();
    path
}

/// The block's bounds, and its cover's, in a window `width` by `height`.
fn cover_in(
    cx: &mut VisualTestContext,
    width: f32,
    height: f32,
) -> (Bounds<Pixels>, Bounds<Pixels>) {
    cx.simulate_resize(size(px(width), px(height)));
    (find(cx, "block-0"), find(cx, "sound-cover"))
}

/// Its cover is square and as large as fits with the rest under it,
/// between where the header and the controls go: as wide as the block
/// lets it be in a tall block, as tall as the rest leaves in a wide one,
/// and never less than its least size. It grows as the window does.
fn cover_fits_its_block(cx: &mut VisualTestContext) {
    let square = |cover: Bounds<Pixels>| (cover.size.width - cover.size.height).abs() < px(0.5);

    let (block, cover) = cover_in(cx, 900., 400.);
    assert!(square(cover), "{cover:?}");
    assert!(
        (cover.top() - block.top() - NOTE_TOP).abs() < px(0.5),
        "as tall as fits below the header in {block:?}: {cover:?}"
    );
    assert!(cover.size.width < block.size.width - SIDES * 2.);
    let (title, quality) = (find(cx, "sound-title"), find(cx, "sound-quality"));
    assert!(title.top() > cover.bottom(), "the rest under it: {title:?}");
    let seek = find(cx, "seek");
    assert!(
        quality.bottom() < seek.top(),
        "above the controls: {quality:?}, {seek:?}"
    );

    let (block, cover) = cover_in(cx, 300., 800.);
    assert!(square(cover), "{cover:?}");
    assert!(
        (cover.left() - block.left() - SIDES).abs() < px(0.5)
            && (block.right() - cover.right() - SIDES).abs() < px(0.5),
        "as wide as {block:?} lets it be: {cover:?}"
    );
    assert!(cover.top() > block.top() + NOTE_TOP + px(1.));

    let (block, cover) = cover_in(cx, 180., 160.);
    assert_eq!(cover.size, size(COVER_MIN_SIZE, COVER_MIN_SIZE));
    let quality = find(cx, "sound-quality");
    assert!(
        quality.left() >= block.left() + SIDES && quality.right() <= block.right() - SIDES,
        "the quality in {block:?}: {quality:?}"
    );

    let mut before = px(0.);
    for step in 0..12 {
        let grown = step as f32 * 60.;
        let (_, cover) = cover_in(cx, 240. + grown, 200. + grown);
        assert!(square(cover), "{cover:?}");
        assert!(
            cover.size.width >= before,
            "{cover:?} smaller than {before:?}"
        );
        before = cover.size.width;
    }
    assert!(before > px(400.), "{before:?}");
}

#[gpui::test]
fn a_sounds_cover_is_as_large_as_fits_its_block(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let path = sound_with_a_cover();
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![path], cx)
        })
        .unwrap();
    let shown = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                workspace
                    .blocks
                    .first()
                    .is_some_and(|block| block.view.read(cx).player().cover().is_some())
            })
            .unwrap()
    });
    assert!(shown, "its cover shows");
    cover_fits_its_block(&mut cx);
    assert_eq!(
        find(&mut cx, "sound-cover-art"),
        find(&mut cx, "sound-cover")
    );
    assert!(!shows(&mut cx, "sound-no-cover"));
}

/// The circle standing in for a missing cover grows and shrinks as a
/// cover would, in the middle of its room, and its icon with it, in the
/// middle of the circle.
#[gpui::test]
fn a_sound_with_no_cover_has_a_circle_as_large_as_fits_its_block(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["tone.m4a"], false);
    cover_fits_its_block(&mut cx);
    for (width, height) in [(900., 400.), (300., 800.)] {
        let (_, room) = cover_in(&mut cx, width, height);
        let circle = find(&mut cx, "sound-no-cover");
        assert!(
            (circle.size.width - room.size.width * NO_COVER_SCALE).abs() < px(0.5)
                && (circle.size.height - circle.size.width).abs() < px(0.5),
            "{circle:?} in {room:?}"
        );
        assert!((circle.center() - room.center()).x.abs() < px(0.5));
        assert!((circle.center() - room.center()).y.abs() < px(0.5));
        // Within a pixel, as its edges are put on whole ones.
        let icon = find(&mut cx, "sound-no-cover-icon");
        assert!(
            (icon.size.width - circle.size.width * NO_COVER_ICON_SCALE).abs() < px(1.)
                && (icon.size.height - icon.size.width).abs() < px(1.),
            "{icon:?} in {circle:?}"
        );
        assert!((icon.center() - circle.center()).x.abs() < px(1.));
        assert!((icon.center() - circle.center()).y.abs() < px(1.));
    }
    assert!(!shows(&mut cx, "sound-cover-art"));
}

/// A sound plays over a speaker filling its block, in white, whose cone
/// moves with its level; a video over none.
#[gpui::test]
fn a_sound_plays_over_a_speaker_moving_with_its_level(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["tone.m4a"], true);
    let block = window
        .read_with(&cx, |workspace, _| workspace.blocks[0].view.clone())
        .unwrap();
    block.update(&mut cx, |view, cx| {
        view.set_level(0.8);
        cx.notify();
    });
    let bounds = find(&mut cx, "block-0");
    let level = cx.update(|_, cx| block.read(cx).level());
    assert!(level > 0.1, "{level}");
    let painted = cx.update(|window, _| window.painted_visualizers());
    assert_eq!(
        painted,
        [(
            bounds,
            VisualizerShader::Speaker { level },
            white().opacity(SPEAKER_OPACITY)
        )]
    );

    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![fixture("clip.mp4")], cx)
        })
        .unwrap();
    let opened = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                workspace.blocks.len() == 2 && workspace.blocks[1].view.read(cx).player().is_known()
            })
            .unwrap()
    });
    assert!(opened, "the video opens");
    find(&mut cx, "block-1");
    let painted = cx.update(|window, _| window.painted_visualizers());
    assert_eq!(painted.len(), 1, "for the sound alone: {painted:?}");
}

/// How many visualizers the window draws now.
fn visualizers(cx: &mut VisualTestContext) -> usize {
    find(cx, "block-0");
    cx.update(|window, _| window.painted_visualizers().len())
}

/// As Settings say, a sound plays over no visualizer, and its quality
/// doesn't show, while the rest does.
#[gpui::test]
fn settings_take_a_sounds_speaker_and_quality_away(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["tune.flac"], false);
    assert_eq!(visualizers(&mut cx), 1, "the speaker");
    assert!(shows(&mut cx, "sound-quality"));

    cx.update(|_, cx| {
        Settings::update(cx, false, |s| {
            s.music_mode.visualizer = Visualizer::None;
        });
    });
    assert_eq!(visualizers(&mut cx), 0);
    assert!(shows(&mut cx, "sound-quality"));

    cx.update(|_, cx| Settings::update(cx, false, |s| s.music_mode.show_quality = false));
    assert!(!shows(&mut cx, "sound-quality"));
    for selector in ["sound-title", "sound-artist", "sound-album"] {
        assert!(shows(&mut cx, selector), "{selector}");
    }
    assert_eq!(visualizers(&mut cx), 0);

    cx.update(|_, cx| {
        Settings::update(cx, false, |s| {
            s.music_mode.visualizer = Visualizer::Speaker;
        });
    });
    assert_eq!(visualizers(&mut cx), 1);
}
