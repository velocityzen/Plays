//! Casting a tile to the DLNA crate's fake renderer, found as on the
//! network, as someone at the screen would. The stream server is the real
//! one, serving this computer only.

use std::sync::Arc;
use std::time::Duration;

use dlna::fake::{FakeRenderer, Kind};
use gpui::{TestAppContext, VisualTestContext, WindowHandle};

use super::casting::{get, tile, wait};
use super::open;
use crate::cast::dlna::DlnaTarget;
use crate::cast::{Casting, Shows};
use crate::media::tracks::TrackKind;
use crate::ui::testing::{self, click, press, shows};
use crate::ui::workspace::Workspace;

/// The app with a DLNA target that has found `renderer`, and `file` open
/// in its window, playing or paused.
fn start(
    cx: &mut TestAppContext,
    renderer: &FakeRenderer,
    file: &str,
    playing: bool,
) -> (WindowHandle<Workspace>, VisualTestContext) {
    let (window, mut cx) = testing::start(cx);
    let target = DlnaTarget::start_at(renderer.search_at());
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(target)])));
    wait(&mut cx, "the renderer to be found", |cx| {
        cx.update(|_, cx| {
            Casting::refresh(cx);
            !Casting::devices(cx).is_empty()
        })
    });
    open(window, &mut cx, &[file], playing);
    if !playing {
        tile(window, &cx).update(&mut cx, |view, cx| {
            view.pause();
            cx.notify();
        });
    }
    (window, cx)
}

/// How many times the renderer was asked to do `action`.
fn asked(renderer: &FakeRenderer, action: &str) -> usize {
    renderer.state().received(action).len()
}

/// What the renderer was last asked of `action`, by `arg`.
fn last_arg(renderer: &FakeRenderer, action: &str, arg: &str) -> Option<String> {
    let state = renderer.state();
    let calls = state.received(action);
    calls
        .last()
        .and_then(|call| call.arg(arg))
        .map(str::to_owned)
}

/// The tile's cast to the renderer, from its menu, and wait for it to be
/// given the media.
fn cast(cx: &mut VisualTestContext, renderer: &FakeRenderer) {
    click(cx, "cast");
    click(cx, "cast-0");
    wait(cx, "the renderer to be given it", |_| {
        asked(renderer, "SetAVTransportURI") > 0
    });
}

#[gpui::test]
fn picking_a_tv_casts_the_front_tile_there_from_where_it_is_with_subtitles(
    cx: &mut TestAppContext,
) {
    let tv = FakeRenderer::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, "clip-subs.mkv", true);
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx);
        view.seek(1.0);
    });

    cast(&mut cx, &tv);
    wait(&mut cx, "the TV to play", |_| asked(&tv, "Play") > 0);
    let (uri, metadata) = {
        let state = tv.state();
        (state.uri.clone(), state.metadata.clone())
    };
    let actions: Vec<String> = tv
        .state()
        .calls
        .iter()
        .map(|call| call.action.clone())
        .filter(|action| ["SetAVTransportURI", "Seek", "Play"].contains(&action.as_str()))
        .collect();
    assert_eq!(
        actions,
        ["SetAVTransportURI", "Seek", "Play"],
        "it goes where the tile was, then plays"
    );
    assert_eq!(last_arg(&tv, "Seek", "Target").as_deref(), Some("0:00:01"));
    assert_eq!(
        view.read_with(&cx, |view, _| view
            .casting()
            .map(|device| device.name.clone())),
        Some("[TV] Fake".to_owned())
    );

    let (status, body) = get(&uri);
    assert_eq!(status, 200);
    let file = std::fs::read(testing::fixture("clip-subs.mkv")).unwrap();
    assert_eq!(body, file, "a Matroska file the TV lists goes as it is");
    assert!(
        metadata.contains("protocolInfo=\"http-get:*:video/x-matroska:DLNA.ORG_OP=01"),
        "{metadata}"
    );

    let srt = metadata
        .split("sec:CaptionInfoEx")
        .nth(1)
        .and_then(|rest| rest.split('>').nth(1))
        .and_then(|rest| rest.split('<').next())
        .expect("the subtitles as Samsung's TVs take them")
        .to_owned();
    assert!(srt.ends_with(".srt"), "{srt}");
    assert!(
        metadata.contains(&format!("protocolInfo=\"http-get:*:text/srt:*\">{srt}<")),
        "beside the media too, as others take them: {metadata}"
    );
    let (status, cues) = get(&srt);
    assert_eq!(status, 200);
    let cues = String::from_utf8(cues).unwrap();
    assert!(
        cues.starts_with("1\n00:00:00,200 --> 00:00:01,000\nFirst\n"),
        "the file's cues, as SRT: {cues}"
    );

    wait(&mut cx, "the tile to say where it plays", |cx| {
        shows(cx, "play-here")
    });
}

#[gpui::test]
fn the_renderers_protocol_info_decides_how_its_served(cx: &mut TestAppContext) {
    let tv = FakeRenderer::start(Kind::Tv);
    // A TV that lists MP4 and MPEG-TS, the latter by the name LG's give it.
    tv.state().sink = "http-get:*:video/mp4:DLNA.ORG_PN=AVC_MP4_HP_HD_AAC,\
                       http-get:*:video/mpeg:DLNA.ORG_PN=MPEG_TS_HD_NA_ISO"
        .into();
    let (_, mut cx) = start(cx, &tv, "clip-subs.mkv", false);

    cast(&mut cx, &tv);
    let (uri, metadata) = {
        let state = tv.state();
        (state.uri.clone(), state.metadata.clone())
    };
    assert!(
        uri.ends_with("stream.ts"),
        "Matroska it doesn't list: {uri}"
    );
    assert!(
        metadata.contains(
            "protocolInfo=\"http-get:*:video/mpeg:DLNA.ORG_OP=10;DLNA.ORG_CI=1;DLNA.ORG_FLAGS="
        ),
        "a stream of MPEG-TS, by the TV's name for it: {metadata}"
    );
    let (status, stream) = get(&uri);
    assert_eq!(status, 200);
    assert!(stream.len() >= 188 * 10, "{} bytes", stream.len());
    assert!(
        stream.chunks(188).take(10).all(|packet| packet[0] == 0x47),
        "MPEG-TS packets"
    );
}

#[gpui::test]
fn a_speaker_is_offered_for_anything_with_sound_and_gets_a_file_of_it(cx: &mut TestAppContext) {
    let speaker = FakeRenderer::start(Kind::Speaker);
    let (_, mut cx) = start(cx, &speaker, "tone.m4a", false);
    let offered = |cx: &mut VisualTestContext, has_audio| {
        cx.update(|_, cx| Casting::devices_for(cx, has_audio).len())
    };
    assert_eq!(offered(&mut cx, true), 1);
    assert_eq!(offered(&mut cx, false), 0);
    let shown = cx.update(|_, cx| Casting::devices(cx)[0].shows);
    assert_eq!(shown, Shows::Sound, "from its protocol info");

    cast(&mut cx, &speaker);
    let (uri, metadata) = {
        let state = speaker.state();
        (state.uri.clone(), state.metadata.clone())
    };
    assert!(
        metadata.contains("protocolInfo=\"http-get:*:audio/mp4:DLNA.ORG_OP=01"),
        "{metadata}"
    );
    assert!(metadata.contains("object.item.audioItem"), "{metadata}");
    let (status, body) = get(&uri);
    assert_eq!(status, 200);
    assert_eq!(body, std::fs::read(testing::fixture("tone.m4a")).unwrap());
    assert_eq!(
        asked(&speaker, "Play"),
        0,
        "a paused tile stays paused there"
    );
}

#[gpui::test]
fn a_videos_sound_goes_to_a_speaker_as_a_stream_asked_where_it_is_often(cx: &mut TestAppContext) {
    let speaker = FakeRenderer::start(Kind::Speaker);
    let (window, mut cx) = start(cx, &speaker, "clip.mp4", false);
    cast(&mut cx, &speaker);
    let (uri, metadata) = {
        let state = speaker.state();
        (state.uri.clone(), state.metadata.clone())
    };
    assert!(uri.ends_with("sound.flac"), "it lists FLAC, not AAC: {uri}");
    assert!(
        metadata.contains("protocolInfo=\"http-get:*:audio/flac:DLNA.ORG_OP=10;DLNA.ORG_CI=1"),
        "a stream, re-encoded: {metadata}"
    );
    assert!(metadata.contains("object.item.audioItem"), "{metadata}");
    let (status, body) = get(&uri);
    assert_eq!(status, 200);
    assert!(body.starts_with(b"fLaC"), "FLAC");
    assert!(
        tile(window, &cx).read_with(&cx, |view, _| view.player().is_sound_elsewhere()),
        "the picture stays here"
    );

    // The picture follows it, so it's asked where it is far more often
    // than once a second, from the start.
    std::thread::sleep(Duration::from_millis(1000));
    let polls = asked(&speaker, "GetPositionInfo");
    assert!(polls >= 4, "asked {polls} times in its first second");

    // Seen moving, it plays on from when it was first heard where it is,
    // as a renderer moves its position on only now and then.
    let view = tile(window, &cx);
    {
        let mut state = speaker.state();
        state.transport = "PLAYING".into();
        state.position = 0.5;
    }
    wait(&mut cx, "the bar to be where it is", |cx| {
        view.read_with(cx, |view, _| (view.position() - 0.5).abs() < 1e-9)
    });
    speaker.state().position = 0.6;
    wait(&mut cx, "the picture to play with it", |cx| {
        view.read_with(cx, |view, _| view.player().is_playing())
    });
    std::thread::sleep(Duration::from_millis(600));
    let position = view.read_with(&cx, |view, _| view.position());
    assert!(
        position > 1.0,
        "at {position}, from when it was heard at 0.6, not when last asked"
    );
}

#[gpui::test]
fn a_speaker_is_given_the_stream_from_where_the_tile_is_and_again_to_seek(cx: &mut TestAppContext) {
    let speaker = FakeRenderer::start(Kind::Speaker);
    let (window, mut cx) = start(cx, &speaker, "clip.mp4", false);
    let view = tile(window, &cx);
    view.update(&mut cx, |view, _| view.seek(1.0));
    cast(&mut cx, &speaker);
    let (uri, metadata) = {
        let state = speaker.state();
        (state.uri.clone(), state.metadata.clone())
    };
    assert!(uri.ends_with("sound.flac?start=1.000"), "{uri}");
    assert!(
        metadata.contains("duration=\"0:00:01.000\""),
        "as long as what's left: {metadata}"
    );
    assert_eq!(asked(&speaker, "Seek"), 0, "it can't seek in a stream");
    let (status, body) = get(&uri);
    assert_eq!(status, 200);
    assert!(body.starts_with(b"fLaC"));

    // Played, it says where it is from where it started, and how long it
    // is as how much it has played.
    press(&mut cx, "space");
    wait(&mut cx, "it to play", |_| asked(&speaker, "Play") == 1);
    {
        let mut state = speaker.state();
        state.position = 0.25;
        state.duration = 0.25;
    }
    wait(&mut cx, "the bar to be where it is", |cx| {
        view.read_with(cx, |view, _| (view.position() - 1.25).abs() < 1e-9)
    });
    assert_eq!(
        view.read_with(&cx, |view, _| view.duration()),
        Some(2.0),
        "the file's"
    );

    // Sought, it's given it again from there, playing as it was. Slow to
    // answer, it says where it was in what it played before for a while,
    // which isn't taken to be from where it starts now.
    speaker.state().answer_after = Duration::from_millis(150);
    view.update(&mut cx, |view, _| view.seek(0.5));
    let mut seen = Vec::new();
    wait(&mut cx, "it to play it from there", |cx| {
        seen.push(view.read_with(cx, |view, _| view.position()));
        asked(&speaker, "Play") == 2
    });
    let polls = asked(&speaker, "GetPositionInfo");
    wait(&mut cx, "it to say where it is since", |cx| {
        seen.push(view.read_with(cx, |view, _| view.position()));
        asked(&speaker, "GetPositionInfo") >= polls + 2
    });
    speaker.state().answer_after = Duration::ZERO;
    assert!(
        seen.iter().all(|position| (position - 0.5).abs() < 0.1),
        "at {seen:?}, from 0.5"
    );
    let uri = speaker.state().uri.clone();
    assert!(uri.ends_with("sound.flac?start=0.500"), "{uri}");
    assert_eq!(asked(&speaker, "Seek"), 0);
}

#[gpui::test]
fn the_tiles_controls_reach_the_tv_and_its_status_moves_the_bar(cx: &mut TestAppContext) {
    let tv = FakeRenderer::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, "clip.mp4", false);
    let view = tile(window, &cx);
    cast(&mut cx, &tv);
    wait(&mut cx, "the tile to say where it plays", |cx| {
        shows(cx, "play-here")
    });

    press(&mut cx, "space");
    wait(&mut cx, "the TV to play", |_| asked(&tv, "Play") > 0);
    let seeks = asked(&tv, "Seek");
    press(&mut cx, "right");
    wait(&mut cx, "the TV to seek", |_| asked(&tv, "Seek") > seeks);
    press(&mut cx, "m");
    wait(&mut cx, "the TV to mute", |_| tv.state().muted);
    press(&mut cx, "down");
    wait(&mut cx, "the TV to turn down", |_| {
        asked(&tv, "SetVolume") > 0
    });
    let volume = tv.state().volume;
    assert!(
        (1..20).contains(&volume),
        "the tile's volume scales the TV's, which was 20: {volume}"
    );
    press(&mut cx, "space");
    wait(&mut cx, "the TV to pause", |_| asked(&tv, "Pause") > 0);
    assert!(
        !view.read_with(&cx, |view, _| view.player().is_playing()),
        "the shortcuts drive the TV, not the tile's own playback"
    );

    tv.state().position = 1.0;
    wait(&mut cx, "the bar to follow the TV", |cx| {
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
    let (seeks, plays) = (asked(&tv, "Seek"), asked(&tv, "Play"));
    press(&mut cx, "space");
    wait(&mut cx, "the TV to play again", |_| {
        asked(&tv, "Play") > plays
    });
    assert!(asked(&tv, "Seek") > seeks, "from the start");
    assert_eq!(last_arg(&tv, "Seek", "Target").as_deref(), Some("0:00:00"));
}

#[gpui::test]
fn something_else_playing_on_the_tv_ends_the_cast(cx: &mut TestAppContext) {
    let tv = FakeRenderer::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, "clip.mp4", true);
    let view = tile(window, &cx);
    cast(&mut cx, &tv);
    wait(&mut cx, "the TV to play", |_| asked(&tv, "Play") > 0);

    tv.play_something_else();
    wait(&mut cx, "the cast to end", |cx| {
        view.read_with(cx, |view, _| view.casting().is_none())
    });
    assert_eq!(
        asked(&tv, "Stop"),
        0,
        "what plays there now is left to play"
    );
}

#[gpui::test]
fn the_tv_going_away_ends_the_cast(cx: &mut TestAppContext) {
    let mut tv = FakeRenderer::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, "clip.mp4", false);
    let view = tile(window, &cx);
    cast(&mut cx, &tv);
    wait(&mut cx, "the tile to say where it plays", |cx| {
        shows(cx, "play-here")
    });

    tv.go_away();
    wait(&mut cx, "the cast to end", |cx| {
        view.read_with(cx, |view, _| view.casting().is_none())
    });
}

#[gpui::test]
fn subtitles_picked_on_the_tile_load_it_again_from_where_it_is(cx: &mut TestAppContext) {
    let tv = FakeRenderer::start(Kind::Tv);
    let (window, mut cx) = start(cx, &tv, "clip-subs.mkv", true);
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx);
    });
    cast(&mut cx, &tv);
    wait(&mut cx, "the TV to play", |_| asked(&tv, "Play") > 0);
    assert!(!tv.state().metadata.contains("CaptionInfo"));
    tv.state().position = 1.0;
    wait(&mut cx, "the bar to follow the TV", |cx| {
        view.read_with(cx, |view, _| view.position() >= 1.0)
    });

    let plays = asked(&tv, "Play");
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx);
    });
    wait(&mut cx, "the TV to be given it again, and play on", |_| {
        asked(&tv, "SetAVTransportURI") == 2 && asked(&tv, "Play") > plays
    });
    assert!(tv.state().metadata.contains("sec:CaptionInfoEx"));
    assert_eq!(last_arg(&tv, "Seek", "Target").as_deref(), Some("0:00:01"));

    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx);
    });
    wait(&mut cx, "the TV to be given it without them", |_| {
        asked(&tv, "SetAVTransportURI") == 3
    });
    assert!(!tv.state().metadata.contains("CaptionInfo"));
}

#[gpui::test]
fn a_tv_that_wont_seek_while_stopped_goes_there_once_it_plays(cx: &mut TestAppContext) {
    let tv = FakeRenderer::start(Kind::Tv);
    // As Rygel's renderers refuse.
    tv.state().refuse.push(("Seek".into(), 711));
    let (window, mut cx) = start(cx, &tv, "clip.mp4", true);
    let view = tile(window, &cx);
    view.update(&mut cx, |view, _| view.seek(1.0));

    cast(&mut cx, &tv);
    wait(&mut cx, "the TV to play", |_| asked(&tv, "Play") > 0);
    // What it says, once asked, rather than what the tile asked for.
    let polls = asked(&tv, "GetPositionInfo");
    wait(&mut cx, "the TV to be asked where it is", |_| {
        asked(&tv, "GetPositionInfo") > polls
    });
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert!(
        view.read_with(&cx, |view, _| view.position()) >= 1.0,
        "the bar stays where the tile was"
    );
    assert_eq!(tv.state().position, 0.0, "it refused");

    tv.state().refuse.clear();
    wait(&mut cx, "the TV to go there", |_| {
        tv.state().position == 1.0
    });
}
