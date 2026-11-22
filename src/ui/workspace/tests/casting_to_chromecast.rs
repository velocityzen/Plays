//! Casting a tile to the Chromecast crate's fake device, as someone at the
//! screen would. The stream server is the real one, serving this computer
//! only.

use std::sync::Arc;

use chromecast::fake::{FakeDevice, Kind};
use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use serde_json::Value;

use super::casting::{get, tile, wait};
use super::open;
use crate::cast::chromecast::ChromecastTarget;
use crate::cast::{Casting, Shows};
use crate::media::tracks::TrackKind;
use crate::settings::{Rgb, Settings, SubtitleEdge};
use crate::ui::testing::{self, click, press, shows};
use crate::ui::workspace::Workspace;

const MEDIA: &str = "urn:x-cast:com.google.cast.media";
const RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";

/// The app with a Chromecast target that has found `device`, and `file`
/// open in its window, playing or paused.
fn start(
    cx: &mut TestAppContext,
    device: &FakeDevice,
    file: &str,
    playing: bool,
) -> (WindowHandle<Workspace>, VisualTestContext) {
    let (window, mut cx) = testing::start(cx);
    let target = ChromecastTarget::with(vec![device.device()]);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(target)])));
    open(window, &mut cx, &[file], playing);
    if !playing {
        tile(window, &cx).update(&mut cx, |view, cx| {
            view.pause();
            cx.notify();
        });
    }
    (window, cx)
}

/// What it was sent on the media channel of `kind`, in order.
fn sent(device: &FakeDevice, kind: &str) -> Vec<Value> {
    device
        .state()
        .received(MEDIA, kind)
        .into_iter()
        .map(|message| message.payload.clone())
        .collect()
}

/// The last of those.
fn last(device: &FakeDevice, kind: &str) -> Value {
    sent(device, kind).pop().unwrap_or_default()
}

/// The tile's cast to the device, from its menu, and wait for it to be
/// given the media.
fn cast(cx: &mut VisualTestContext, device: &FakeDevice) {
    click(cx, "cast");
    click(cx, "cast-0");
    wait(cx, "the device to be given it", |_| {
        !sent(device, "LOAD").is_empty()
    });
}

#[gpui::test]
fn picking_a_tv_casts_the_front_tile_there_from_where_it_is_with_styled_subtitles(
    cx: &mut TestAppContext,
) {
    let tv = FakeDevice::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, "clip-subs.mkv", true);
    cx.update(|_, cx| {
        let subtitles = &mut cx.global_mut::<Settings>().subtitles;
        subtitles.size = 1.5;
        subtitles.color = Rgb(0xffcc00);
        subtitles.edge = SubtitleEdge::Shadow;
        subtitles.background_opacity = 0.5;
    });
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx);
        view.seek(1.0);
    });

    cast(&mut cx, &tv);
    let load = last(&tv, "LOAD");
    // It played on a little before it was cast.
    let here = view.read_with(&cx, |view, _| view.player().position());
    let start = load["currentTime"].as_f64().unwrap();
    assert!(
        start >= 1.0 && (start - here).abs() < 0.01,
        "from where the tile was: {start} against {here}"
    );
    assert_eq!(load["autoplay"], true);
    assert_eq!(
        view.read_with(&cx, |view, _| view
            .casting()
            .map(|device| device.name.clone())),
        Some("Fake TV".to_owned())
    );

    let media = &load["media"];
    let url = media["contentId"].as_str().unwrap().to_owned();
    assert!(url.ends_with("master.m3u8"), "Matroska goes as HLS: {url}");
    assert_eq!(media["hlsSegmentFormat"], "ts", "{media}");
    assert_eq!(media["hlsVideoSegmentFormat"], "mpeg2_ts");
    assert_eq!(media["streamType"], "BUFFERED");
    let (status, master) = get(&url);
    assert_eq!(status, 200);
    let master = String::from_utf8(master).unwrap();
    assert!(
        !master.contains("SUBTITLES"),
        "they go beside it, not in it too: {master}"
    );

    let track = &media["tracks"][0];
    assert_eq!(track["trackContentType"], "text/vtt");
    assert_eq!(track["language"], "en", "as BCP 47 has it");
    assert_eq!(track["name"], "English");
    assert_eq!(load["activeTrackIds"], serde_json::json!([1]));
    let (status, vtt) = get(track["trackContentId"].as_str().unwrap());
    assert_eq!(status, 200);
    let vtt = String::from_utf8(vtt).unwrap();
    assert!(
        vtt.starts_with("WEBVTT") && vtt.contains("First"),
        "the file's cues: {vtt}"
    );

    let style = &media["textTrackStyle"];
    assert_eq!(style["fontScale"], 1.5, "{style}");
    assert_eq!(style["foregroundColor"], "#FFCC00FF");
    assert_eq!(style["edgeType"], "DROP_SHADOW");
    assert_eq!(style["backgroundColor"], "#00000080");

    wait(&mut cx, "the tile to say where it plays", |cx| {
        shows(cx, "play-here")
    });
}

#[gpui::test]
fn a_speaker_is_offered_for_anything_with_sound_and_gets_a_file_of_it(cx: &mut TestAppContext) {
    let speaker = FakeDevice::start(Kind::Speaker);
    let (_, mut cx) = start(cx, &speaker, "tone.m4a", false);
    let offered = |cx: &mut VisualTestContext, has_audio| {
        cx.update(|_, cx| Casting::devices_for(cx, has_audio).len())
    };
    assert_eq!(offered(&mut cx, true), 1);
    assert_eq!(offered(&mut cx, false), 0);
    let shown = cx.update(|_, cx| Casting::devices(cx)[0].shows);
    assert_eq!(shown, Shows::Sound);

    cast(&mut cx, &speaker);
    let load = last(&speaker, "LOAD");
    assert_eq!(load["media"]["contentType"], "audio/mp4");
    assert_eq!(load["autoplay"], false, "a paused tile stays paused there");
    assert!(load["media"].get("hlsSegmentFormat").is_none());
    let (status, body) = get(load["media"]["contentId"].as_str().unwrap());
    assert_eq!(status, 200);
    assert_eq!(body, std::fs::read(testing::fixture("tone.m4a")).unwrap());
}

#[gpui::test]
fn a_videos_sound_goes_to_a_speaker_as_hls_of_the_sound_alone(cx: &mut TestAppContext) {
    let speaker = FakeDevice::start(Kind::Speaker);
    let (window, mut cx) = start(cx, &speaker, "clip.mp4", false);
    cast(&mut cx, &speaker);
    let media = last(&speaker, "LOAD")["media"].clone();
    let url = media["contentId"].as_str().unwrap().to_owned();
    assert!(url.ends_with("master.m3u8"), "{url}");
    let (status, master) = get(&url);
    assert_eq!(status, 200);
    let master = String::from_utf8(master).unwrap();
    assert!(
        master.contains("mp4a.40.2") && !master.contains("avc1"),
        "the sound alone: {master}"
    );
    assert!(
        tile(window, &cx).read_with(&cx, |view, _| view.player().is_sound_elsewhere()),
        "the picture stays here"
    );
}

#[gpui::test]
fn the_tiles_controls_reach_it_and_its_status_moves_the_bar(cx: &mut TestAppContext) {
    let tv = FakeDevice::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, "clip.mp4", false);
    let view = tile(window, &cx);
    cast(&mut cx, &tv);
    let load = last(&tv, "LOAD");
    assert_eq!(load["media"]["contentType"], "video/mp4");
    let (_, body) = get(load["media"]["contentId"].as_str().unwrap());
    assert_eq!(
        body,
        std::fs::read(testing::fixture("clip.mp4")).unwrap(),
        "an MP4 file goes as it is"
    );
    wait(&mut cx, "the tile to say where it plays", |cx| {
        shows(cx, "play-here")
    });

    press(&mut cx, "space");
    wait(&mut cx, "it to play", |_| !sent(&tv, "PLAY").is_empty());
    press(&mut cx, "right");
    wait(&mut cx, "it to seek", |_| !sent(&tv, "SEEK").is_empty());
    press(&mut cx, "m");
    wait(&mut cx, "what it plays to mute", |_| {
        tv.state()
            .playing
            .as_ref()
            .is_some_and(|playing| playing.muted)
    });
    press(&mut cx, "down");
    wait(&mut cx, "what it plays to turn down", |_| {
        tv.state()
            .playing
            .as_ref()
            .is_some_and(|playing| playing.volume < 1.0)
    });
    {
        let state = tv.state();
        assert_eq!(state.volume, 0.5, "the device's own volume stays");
        assert!(!state.muted);
        assert!(state.received(RECEIVER, "SET_VOLUME").is_empty());
    }
    press(&mut cx, "space");
    wait(&mut cx, "it to pause", |_| !sent(&tv, "PAUSE").is_empty());
    assert!(
        !view.read_with(&cx, |view, _| view.player().is_playing()),
        "the shortcuts drive it, not the tile's own playback"
    );

    tv.state().playing.as_mut().unwrap().position = 1.0;
    wait(&mut cx, "the bar to follow it", |cx| {
        view.read_with(cx, |view, _| (view.position() - 1.0).abs() < 1e-9)
    });
    assert!(!view.read_with(&cx, |view, _| view.is_playing()));

    tv.finish();
    wait(&mut cx, "the bar to show the end", |cx| {
        view.read_with(cx, |view, _| {
            !view.is_playing() && (view.position() - 2.0).abs() < 1e-9
        })
    });
    assert!(
        view.read_with(&cx, |view, _| view.casting().is_some()),
        "the cast goes on at the end"
    );
    // Sent back from the end, it plays from there once told to.
    view.update(&mut cx, |view, _| view.seek(1.0));
    wait(&mut cx, "the bar to go there", |cx| {
        view.read_with(cx, |view, _| (view.position() - 1.0).abs() < 1e-9)
    });
    assert_eq!(sent(&tv, "LOAD").len(), 1, "not till it plays");
    press(&mut cx, "space");
    wait(&mut cx, "it to be given it again", |_| {
        sent(&tv, "LOAD").len() == 2
    });
    let again = last(&tv, "LOAD");
    assert_eq!(again["currentTime"], 1.0, "from where it was sent");
    assert_eq!(again["autoplay"], true);
    wait(&mut cx, "the stream to be turned down again", |_| {
        sent(&tv, "SET_VOLUME").last().is_some_and(|set| {
            set["mediaSessionId"] == tv.state().playing.as_ref().unwrap().media_session
        })
    });

    tv.finish();
    wait(&mut cx, "the bar to show the end", |cx| {
        view.read_with(cx, |view, _| (view.position() - 2.0).abs() < 1e-9)
    });
    press(&mut cx, "space");
    wait(&mut cx, "it to be given it again", |_| {
        sent(&tv, "LOAD").len() == 3
    });
    assert_eq!(last(&tv, "LOAD")["currentTime"], 0.0, "from the start");
}

#[gpui::test]
fn subtitles_picked_on_the_tile_load_it_again_then_turn_off_and_on(cx: &mut TestAppContext) {
    let tv = FakeDevice::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, "clip-subs.mkv", true);
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx);
    });
    cast(&mut cx, &tv);
    assert!(last(&tv, "LOAD")["media"].get("tracks").is_none());
    tv.state().playing.as_mut().unwrap().position = 1.0;
    wait(&mut cx, "the bar to follow it", |cx| {
        view.read_with(cx, |view, _| view.position() >= 1.0)
    });

    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx);
    });
    wait(&mut cx, "it to be given it again with them", |_| {
        sent(&tv, "LOAD").len() == 2
    });
    let load = last(&tv, "LOAD");
    assert_eq!(load["currentTime"], 1.0, "from where it was");
    assert_eq!(load["autoplay"], true, "playing on");
    assert_eq!(load["activeTrackIds"], serde_json::json!([1]));
    let url = load["media"]["tracks"][0]["trackContentId"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        url.ends_with("subs.vtt?1"),
        "at a URL it hasn't fetched: {url}"
    );
    let (status, vtt) = get(&url);
    assert_eq!(status, 200);
    assert!(String::from_utf8(vtt).unwrap().contains("First"));

    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx);
    });
    wait(&mut cx, "the track to be turned off", |_| {
        !sent(&tv, "EDIT_TRACKS_INFO").is_empty()
    });
    assert_eq!(
        last(&tv, "EDIT_TRACKS_INFO")["activeTrackIds"],
        serde_json::json!([])
    );
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx);
    });
    wait(&mut cx, "the track to be turned on", |_| {
        sent(&tv, "EDIT_TRACKS_INFO").len() == 2
    });
    let edit = last(&tv, "EDIT_TRACKS_INFO");
    assert_eq!(edit["activeTrackIds"], serde_json::json!([1]));
    assert!(edit["textTrackStyle"]["fontScale"].is_number(), "{edit}");
    assert_eq!(
        sent(&tv, "LOAD").len(),
        2,
        "the same subtitles turn off and on without loading again"
    );
}

#[gpui::test]
fn other_subtitles_load_it_again_with_them(cx: &mut TestAppContext) {
    // The Matroska file with a French SRT beside it.
    let dir = crate::files::test_folder("chromecast");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("clip-subs.mkv");
    std::fs::copy(testing::fixture("clip-subs.mkv"), &file).unwrap();
    std::fs::write(
        dir.join("clip-subs.fr.srt"),
        "1\n00:00:00,200 --> 00:00:01,000\nPremier\n",
    )
    .unwrap();
    let tv = FakeDevice::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, file.to_str().unwrap(), true);
    let view = tile(window, &cx);
    let beside = view.read_with(&cx, |view, _| {
        let info = view.player().info().unwrap();
        info.tracks(TrackKind::Subtitle)
            .find(|track| track.file.is_some())
            .map(|track| track.index)
            .expect("the SRT beside it")
    });
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx);
    });
    cast(&mut cx, &tv);
    let track = |load: &Value| load["media"]["tracks"][0].clone();
    assert_eq!(track(&last(&tv, "LOAD"))["language"], "en");

    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(beside), cx);
    });
    wait(&mut cx, "it to be given it again with them", |_| {
        sent(&tv, "LOAD").len() == 2
    });
    let french = track(&last(&tv, "LOAD"));
    assert_eq!(french["language"], "fr");
    let (_, vtt) = get(french["trackContentId"].as_str().unwrap());
    assert!(
        String::from_utf8(vtt).unwrap().contains("Premier"),
        "the file's cues, not the track's"
    );
    assert!(sent(&tv, "EDIT_TRACKS_INFO").is_empty());
}

/// Cast `clip.mp4` to a TV and end it there as `end` does: the tile's
/// cast ends too, leaving what plays there as it is.
fn ends_the_cast(cx: &mut TestAppContext, end: impl FnOnce(&FakeDevice)) {
    let tv = FakeDevice::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, "clip.mp4", true);
    let view = tile(window, &cx);
    cast(&mut cx, &tv);
    wait(&mut cx, "the tile to say where it plays", |cx| {
        shows(cx, "play-here")
    });

    end(&tv);
    wait(&mut cx, "the cast to end", |cx| {
        view.read_with(cx, |view, _| view.casting().is_none())
    });
    assert!(
        tv.state().received(RECEIVER, "STOP").is_empty(),
        "what's there now is left to play"
    );
}

#[gpui::test]
fn something_else_cast_there_ends_the_cast(cx: &mut TestAppContext) {
    ends_the_cast(cx, FakeDevice::replace);
}

#[gpui::test]
fn the_app_closing_there_ends_the_cast(cx: &mut TestAppContext) {
    ends_the_cast(cx, FakeDevice::close_app);
}

#[gpui::test]
fn another_app_launched_there_ends_the_cast(cx: &mut TestAppContext) {
    ends_the_cast(cx, FakeDevice::launch_another_app);
}

#[gpui::test]
fn the_device_going_away_ends_the_cast(cx: &mut TestAppContext) {
    ends_the_cast(cx, FakeDevice::go_away);
}

/// What it plays goes idle, for `reason`.
fn idle(tv: &FakeDevice, reason: &'static str) {
    let mut state = tv.state();
    let playing = state.playing.as_mut().unwrap();
    playing.state = "IDLE";
    playing.idle_reason = Some(reason);
}

#[gpui::test]
fn it_stopped_there_ends_the_cast(cx: &mut TestAppContext) {
    ends_the_cast(cx, |tv| idle(tv, "CANCELLED"));
}

#[gpui::test]
fn it_failing_to_play_it_ends_the_cast(cx: &mut TestAppContext) {
    ends_the_cast(cx, |tv| idle(tv, "ERROR"));
}
