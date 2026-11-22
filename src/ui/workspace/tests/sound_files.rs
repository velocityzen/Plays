//! A sound file's block: under its cover, its title, who made it, its
//! album and its quality.

use gpui::TestAppContext;

use super::{drawn_in, open};
use crate::ui::testing::{self, shows};

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
