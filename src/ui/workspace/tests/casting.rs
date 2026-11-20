//! Casting a tile to a fake device, as someone at the screen would. The
//! stream server is the real one, serving this computer only, so what it
//! tells the device to fetch is fetched.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle, point,
};

use super::open;
use crate::cast::fake::{Call, FakeTarget, KIND};
use crate::cast::target::{Load, State};
use crate::cast::{Casting, Device, Shows};
use crate::media::tracks::TrackKind;
use crate::settings::Settings;
use crate::ui::pip::{self, PipEvent};
use crate::ui::player::PlayerView;
use crate::ui::testing::{self, OPEN_WAIT, click, find, hover, press, shows};
use crate::ui::theme::motion::CAST_POLL;
use crate::ui::workspace::Workspace;

/// The app with a fake protocol that has found a TV, and `file` open in
/// its window, playing or paused.
fn start(
    cx: &mut TestAppContext,
    file: &str,
    playing: bool,
) -> (
    WindowHandle<Workspace>,
    VisualTestContext,
    FakeTarget,
    Device,
) {
    let (window, mut cx) = testing::start(cx);
    let fake = FakeTarget::default();
    let tv = fake.add("Living Room", Shows::Video);
    cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
    open(window, &mut cx, &[file], playing);
    if !playing {
        tile(window, &cx).update(&mut cx, |view, cx| {
            view.pause();
            cx.notify();
        });
    }
    (window, cx, fake, tv)
}

pub(super) fn tile(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Entity<PlayerView> {
    window
        .read_with(cx, |workspace, _| workspace.tiles[0].view.clone())
        .unwrap()
}

fn calls(fake: &FakeTarget, device: &Device) -> Vec<Call> {
    fake.screen(device).lock().calls.clone()
}

fn loads(fake: &FakeTarget, device: &Device) -> Vec<Load> {
    calls(fake, device)
        .into_iter()
        .filter_map(|call| match call {
            Call::Load(load) => Some(load),
            _ => None,
        })
        .collect()
}

/// Have the device say it is at `position`, doing `state`.
fn say(fake: &FakeTarget, device: &Device, state: State, position: f64) {
    let screen = fake.screen(device);
    let mut screen = screen.lock();
    screen.status.state = state;
    screen.status.position = position;
}

/// Run until `done`, moving the clock on so the tile follows its device,
/// and giving the link's thread real time, for at most `OPEN_WAIT`.
pub(super) fn wait(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let start = Instant::now();
    while start.elapsed() < OPEN_WAIT {
        cx.executor().advance_clock(CAST_POLL);
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("{what} didn't happen in {OPEN_WAIT:?}");
}

/// Cast the tile to `device`, and wait for it to play there.
fn cast(
    window: WindowHandle<Workspace>,
    cx: &mut VisualTestContext,
    fake: &FakeTarget,
    device: &Device,
) -> Load {
    let view = tile(window, cx);
    let to = device.clone();
    view.update(cx, |view, cx| view.cast_to(Some(to), cx));
    wait(cx, "the device to load it", |_| {
        !loads(fake, device).is_empty()
    });
    loads(fake, device).remove(0)
}

/// Whether anything listens where `url` is.
fn listening(url: &str) -> bool {
    let rest = url.strip_prefix("http://").expect("an http:// URL");
    let host = rest.split('/').next().expect("a host");
    TcpStream::connect(host).is_ok()
}

/// What `url` serves: its status and body.
pub(super) fn get(url: &str) -> (u16, Vec<u8>) {
    let rest = url.strip_prefix("http://").expect("an http:// URL");
    let (host, path) = rest.split_once('/').expect("a path");
    let mut stream = TcpStream::connect(host).expect("the stream server");
    write!(
        stream,
        "GET /{path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a response head");
    let head = String::from_utf8_lossy(&response[..end]);
    let status = head.split(' ').nth(1).unwrap().parse().unwrap();
    (status, response[end + 4..].to_vec())
}

#[gpui::test]
fn picking_a_device_plays_the_file_there_from_where_it_is_and_pauses_the_tile(
    cx: &mut TestAppContext,
) {
    let (window, mut cx, fake, tv) = start(cx, "clip.mp4", true);
    let view = tile(window, &cx);

    click(&mut cx, "cast");
    click(&mut cx, "cast-0");
    wait(&mut cx, "the device to load it", |_| {
        !loads(&fake, &tv).is_empty()
    });

    let load = loads(&fake, &tv).remove(0);
    let (casting, playing_here, position_here) = view.read_with(&cx, |view, _| {
        (
            view.casting().cloned(),
            view.player().is_playing(),
            view.player().position(),
        )
    });
    assert_eq!(casting, Some(tv.clone()));
    assert!(!playing_here, "the tile waits while the device plays");
    assert!(
        (load.start - position_here).abs() < 0.01,
        "it starts where the tile was: {} against {position_here}",
        load.start
    );
    assert!(load.autoplay, "it plays there, as it played here");
    assert_eq!(load.mime, "video/mp4");
    assert_eq!(load.duration, Some(2.0));
    let (status, body) = get(&load.url);
    assert_eq!(status, 200);
    let file = std::fs::read(testing::fixture("clip.mp4")).unwrap();
    assert_eq!(body, file, "an MP4 the device plays goes as it is");

    wait(&mut cx, "the tile to say where it plays", |cx| {
        shows(cx, "play-here")
    });
}

#[gpui::test]
fn the_tiles_controls_reach_the_device_and_its_status_moves_the_bar(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip.mp4", false);
    let view = tile(window, &cx);
    let load = cast(window, &mut cx, &fake, &tv);
    assert!(!load.autoplay, "a paused tile stays paused there");

    press(&mut cx, "space");
    wait(&mut cx, "the device to play", |_| {
        calls(&fake, &tv).contains(&Call::Play)
    });
    press(&mut cx, "right");
    wait(&mut cx, "the device to seek", |_| {
        calls(&fake, &tv)
            .iter()
            .any(|call| matches!(call, Call::Seek(_)))
    });
    press(&mut cx, "m");
    wait(&mut cx, "the device to mute", |_| {
        calls(&fake, &tv)
            .iter()
            .any(|call| matches!(call, Call::Volume(_, true)))
    });
    press(&mut cx, "space");
    wait(&mut cx, "the device to pause", |_| {
        calls(&fake, &tv).contains(&Call::Pause)
    });
    assert!(
        !view.read_with(&cx, |view, _| view.player().is_playing()),
        "the shortcuts drive the device, not the tile's own playback"
    );

    say(&fake, &tv, State::Paused, 1.25);
    wait(&mut cx, "the bar to follow the device", |cx| {
        view.read_with(cx, |view, _| (view.position() - 1.25).abs() < 1e-9)
    });
}

/// What a Picture in Picture window's button asks of `view`.
fn pip_button(cx: &mut VisualTestContext, view: &Entity<PlayerView>, event: PipEvent) {
    view.update_in(cx, |view, window, cx| view.on_pip_event(event, window, cx));
}

#[gpui::test]
fn picture_in_pictures_buttons_drive_the_device_and_it_isnt_offered(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip.mp4", false);
    let view = tile(window, &cx);
    if pip::supported() {
        wait(&mut cx, "a picture", |cx| {
            view.read_with(cx, |view, _| view.player().frame().is_some())
        });
        hover(&mut cx, "tile-0");
        assert!(shows(&mut cx, "pip"), "offered while it plays here");
    }
    cast(window, &mut cx, &fake, &tv);
    if pip::supported() {
        hover(&mut cx, "tile-0");
        assert!(!shows(&mut cx, "pip"), "no picture here to show");
    }

    pip_button(&mut cx, &view, PipEvent::SetPlaying(true));
    wait(&mut cx, "the device to play", |_| {
        calls(&fake, &tv).contains(&Call::Play)
    });
    pip_button(&mut cx, &view, PipEvent::Skip(1.0));
    wait(&mut cx, "the device to seek", |_| {
        calls(&fake, &tv)
            .iter()
            .any(|call| matches!(call, Call::Seek(_)))
    });
    pip_button(&mut cx, &view, PipEvent::SetPlaying(false));
    wait(&mut cx, "the device to pause", |_| {
        calls(&fake, &tv).contains(&Call::Pause)
    });
    assert!(
        !view.read_with(&cx, |view, _| view.player().is_playing()),
        "the tile's own playback stays paused"
    );
}

#[gpui::test]
fn picture_in_pictures_buttons_drive_a_speaker_the_sound_is_cast_to(cx: &mut TestAppContext) {
    let (window, mut cx, fake, _) = start(cx, "clip.mp4", false);
    let speaker = speaker(&fake, "Kitchen");
    let view = tile(window, &cx);
    cast(window, &mut cx, &fake, &speaker);

    pip_button(&mut cx, &view, PipEvent::SetPlaying(true));
    wait(&mut cx, "the speaker to play", |_| {
        calls(&fake, &speaker).contains(&Call::Play)
    });
    say(&fake, &speaker, State::Playing, 0.5);
    pip_button(&mut cx, &view, PipEvent::SetPlaying(false));
    wait(&mut cx, "the speaker to pause", |_| {
        calls(&fake, &speaker).contains(&Call::Pause)
    });
}

#[gpui::test]
fn ending_the_cast_plays_on_here_from_where_the_device_was(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip.mp4", true);
    let view = tile(window, &cx);
    let load = cast(window, &mut cx, &fake, &tv);
    say(&fake, &tv, State::Paused, 1.5);
    wait(&mut cx, "the bar to follow the device", |cx| {
        view.read_with(cx, |view, _| (view.position() - 1.5).abs() < 1e-9)
    });

    wait(&mut cx, "the tile to say where it plays", |cx| {
        shows(cx, "play-here")
    });
    click(&mut cx, "play-here");

    let (casting, position, playing) = view.read_with(&cx, |view, _| {
        (
            view.casting().cloned(),
            view.player().position(),
            view.player().is_playing(),
        )
    });
    assert_eq!(casting, None);
    assert!((position - 1.5).abs() < 0.05, "it goes on at {position}");
    assert!(!playing, "paused, as the device was");
    wait(&mut cx, "the device to stop", |_| {
        calls(&fake, &tv).contains(&Call::Stop)
    });
    // With no cast left, the stream server stops listening.
    wait(&mut cx, "the stream server to stop", |_| {
        !listening(&load.url)
    });
}

#[gpui::test]
fn a_device_going_away_ends_the_cast(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip.mp4", false);
    let view = tile(window, &cx);
    cast(window, &mut cx, &fake, &tv);
    say(&fake, &tv, State::Paused, 0.75);
    wait(&mut cx, "the bar to follow the device", |cx| {
        view.read_with(cx, |view, _| (view.position() - 0.75).abs() < 1e-9)
    });

    fake.remove(&tv);
    wait(&mut cx, "the cast to end", |cx| {
        view.read_with(cx, |view, _| view.casting().is_none())
    });
    let position = view.read_with(&cx, |view, _| view.player().position());
    assert!(
        (position - 0.75).abs() < 0.05,
        "it goes on here from where the device was, not {position}"
    );
    cx.update(|_, cx| Casting::refresh(cx));
    assert!(
        !shows(&mut cx, "cast"),
        "with no devices left, there is nothing to cast to"
    );
}

#[gpui::test]
fn turning_its_protocol_off_leaves_a_cast_going(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip.mp4", false);
    let view = tile(window, &cx);
    cast(window, &mut cx, &fake, &tv);

    cx.update(|_, cx| {
        Settings::update(cx, false, |s| {
            s.casting.look_with.insert(KIND.to_owned(), false);
        })
    });
    cx.run_until_parked();
    assert!(
        cx.update(|_, cx| Casting::devices(cx).is_empty()),
        "nothing more is found"
    );
    press(&mut cx, "space");
    wait(&mut cx, "the device to play", |_| {
        calls(&fake, &tv).contains(&Call::Play)
    });
    assert_eq!(
        view.read_with(&cx, |view, _| view.casting().cloned()),
        Some(tv)
    );
}

#[gpui::test]
fn subtitles_picked_on_the_tile_change_on_the_device(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip-subs.mkv", false);
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx)
    });
    let load = cast(window, &mut cx, &fake, &tv);
    assert_eq!(load.subtitles, None);
    assert!(load.url.ends_with("master.m3u8"), "Matroska goes as HLS");

    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx)
    });
    wait(&mut cx, "the device to get the subtitles", |_| {
        calls(&fake, &tv)
            .iter()
            .any(|call| matches!(call, Call::Subtitles(Some(_))))
    });
    let Some(Call::Subtitles(Some(subtitles))) = calls(&fake, &tv)
        .into_iter()
        .find(|call| matches!(call, Call::Subtitles(Some(_))))
    else {
        unreachable!()
    };
    assert_eq!(subtitles.name, "English");
    assert_eq!(subtitles.language.as_deref(), Some("eng"));
    let (status, vtt) = get(&subtitles.webvtt_url);
    assert_eq!(status, 200);
    let vtt = String::from_utf8(vtt).unwrap();
    assert!(vtt.starts_with("WEBVTT"), "{vtt}");
    assert!(vtt.contains("First"), "the file's cues: {vtt}");

    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx)
    });
    wait(&mut cx, "the device to hide them", |_| {
        calls(&fake, &tv).last() == Some(&Call::Subtitles(None))
    });
    assert_eq!(
        loads(&fake, &tv).len(),
        1,
        "subtitles change without loading again"
    );
}

#[gpui::test]
fn another_track_loads_it_again_with_the_subtitles(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip-subs.mkv", false);
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx)
    });
    let first = cast(window, &mut cx, &fake, &tv);
    let subtitles = first.subtitles.clone().expect("the subtitles go along");

    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Audio, Some(1), cx)
    });
    wait(&mut cx, "it to load again", |_| {
        loads(&fake, &tv).len() == 2
    });
    let again = loads(&fake, &tv).remove(1);
    assert_ne!(again.url, first.url);
    let kept = again.subtitles.expect("the subtitles still go along");
    assert_eq!(kept.name, subtitles.name);
    assert_ne!(kept.webvtt_url, subtitles.webvtt_url, "served with it");
}

const PICTURES_STAY: &str = "Picture subtitles stay on this computer";

fn stays_here(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    tile(window, cx).read_with(cx, |view, cx| view.cast_stays_here(cx))
}

#[gpui::test]
fn picture_subtitles_stay_here_when_the_video_goes_as_it_is(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip-dvdsub.mkv", false);
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx)
    });
    let load = cast(window, &mut cx, &fake, &tv);
    assert_eq!(load.subtitles, None);
    assert!(stays_here(window, &mut cx).is_empty());

    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx)
    });
    wait(&mut cx, "the device to be told", |_| {
        calls(&fake, &tv).last() == Some(&Call::Subtitles(None))
    });
    assert_eq!(loads(&fake, &tv).len(), 1, "the video goes as it is");
    assert_eq!(stays_here(window, &mut cx), [PICTURES_STAY]);
}

#[cfg(target_os = "macos")]
#[gpui::test]
fn picture_subtitles_are_drawn_into_video_it_re_encodes(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip-dvdsub.mkv", false);
    // Too big for the TV, so it is re-encoded.
    fake.screen(&tv).lock().profile = stream_server::Profile {
        max_size: Some((80, 60)),
        ..stream_server::Profile::default()
    };
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx)
    });
    let first = cast(window, &mut cx, &fake, &tv);
    assert!(first.start < 0.01);
    say(&fake, &tv, State::Paused, 1.25);
    wait(&mut cx, "the bar to follow the TV", |cx| {
        view.read_with(cx, |view, _| (view.position() - 1.25).abs() < 1e-9)
    });

    // Picked, they go drawn in: served again, at a URL of its own.
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, Some(2), cx)
    });
    wait(&mut cx, "it to load again", |_| {
        loads(&fake, &tv).len() == 2
    });
    let drawn = loads(&fake, &tv).remove(1);
    assert_ne!(drawn.url, first.url);
    assert_eq!(drawn.subtitles, None, "nothing for the TV to show itself");
    assert!((drawn.start - 1.25).abs() < 0.01, "from where it was");
    assert_eq!(get(&first.url).0, 404, "the first is no longer served");
    assert_eq!(get(&drawn.url).0, 200);
    wait(&mut cx, "the tile to say they go", |cx| {
        !stays_here(window, cx).contains(&PICTURES_STAY)
    });

    // Turned off, it is served again without them.
    view.update(&mut cx, |view, cx| {
        view.select_track(TrackKind::Subtitle, None, cx)
    });
    wait(&mut cx, "it to load again", |_| {
        loads(&fake, &tv).len() == 3
    });
    let plain = loads(&fake, &tv).remove(2);
    assert_ne!(plain.url, drawn.url);
    assert_eq!(get(&drawn.url).0, 404);
}

/// Open the file before (`-1`) or after (`1`) the tile's in its folder,
/// once it knows which they are.
fn open_neighbor(view: &Entity<PlayerView>, cx: &mut VisualTestContext, direction: i32) {
    wait(cx, "the tile to look through its folder", |cx| {
        view.read_with(cx, |view, _| view.knows_neighbors())
    });
    view.update(cx, |view, cx| view.open_neighbor(direction, cx));
}

#[gpui::test]
fn the_next_file_goes_to_the_device_with_the_subtitles_beside_it(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip-subs.mkv", false);
    let view = tile(window, &cx);
    let first = cast(window, &mut cx, &fake, &tv);
    assert_eq!(first.subtitles, None, "none are picked in that one");
    open_neighbor(&view, &mut cx, 1);
    wait(&mut cx, "the next file to load there", |_| {
        loads(&fake, &tv).len() == 2
    });
    let next = loads(&fake, &tv).remove(1);
    let subtitles = next.subtitles.expect("clip.srt goes along");
    let (status, vtt) = get(&subtitles.webvtt_url);
    assert_eq!(status, 200);
    assert!(String::from_utf8_lossy(&vtt).starts_with("WEBVTT"));
    assert_eq!(
        view.read_with(&cx, |view, _| view.casting().cloned()),
        Some(tv)
    );
}

#[gpui::test]
fn the_previous_files_sound_goes_to_the_speaker_its_picture_staying_here(cx: &mut TestAppContext) {
    let (window, mut cx, fake, _) = start(cx, "clip.mp4", false);
    let speaker = speaker(&fake, "Kitchen");
    let view = tile(window, &cx);
    assert!(cast(window, &mut cx, &fake, &speaker).picture_here);
    open_neighbor(&view, &mut cx, -1);
    wait(&mut cx, "the previous file to load there", |_| {
        loads(&fake, &speaker).len() == 2
    });
    let previous = loads(&fake, &speaker).remove(1);
    assert!(previous.picture_here, "only its sound goes");
    assert_eq!(previous.mime, "audio/aac");
    assert!(view.read_with(&cx, |view, _| view.casts_sound_only()));
    assert!(
        !shows(&mut cx, "play-here"),
        "the picture stays on the tile"
    );
}

/// A speaker taking MP4 files, and a stream of AAC for the sound of
/// anything else.
fn speaker(fake: &FakeTarget, name: &str) -> Device {
    let speaker = fake.add(name, Shows::Sound);
    fake.screen(&speaker).lock().profile = stream_server::Profile {
        hls: None,
        hls_subtitles: false,
        sound_stream: vec![stream_server::Container::Aac],
        video: Vec::new(),
        max_size: None,
        ..stream_server::Profile::default()
    };
    speaker
}

#[gpui::test]
fn speakers_are_offered_for_anything_with_sound(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "tone.m4a", false);
    let speaker = speaker(&fake, "Kitchen");
    cx.update(|_, cx| Casting::refresh(cx));
    let offered = |cx: &mut VisualTestContext, has_audio| {
        cx.update(|_, cx| {
            Casting::devices_for(cx, has_audio)
                .into_iter()
                .map(|device| device.name)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(offered(&mut cx, true), ["Kitchen", "Living Room"]);
    assert_eq!(offered(&mut cx, false), ["Living Room"]);

    let load = cast(window, &mut cx, &fake, &speaker);
    assert_eq!(load.mime, "audio/mp4");
    assert!(!load.picture_here, "sound alone goes as it is");
    let (_, body) = get(&load.url);
    assert_eq!(body, std::fs::read(testing::fixture("tone.m4a")).unwrap());
    assert!(loads(&fake, &tv).is_empty());
}

#[gpui::test]
fn a_video_without_sound_doesnt_go_to_a_speaker(cx: &mut TestAppContext) {
    let (window, mut cx, fake, _) = start(cx, "clip-ip.mkv", false);
    let speaker = speaker(&fake, "Kitchen");
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| view.cast_to(Some(speaker.clone()), cx));
    assert_eq!(view.read_with(&cx, |view, _| view.casting().cloned()), None);
    std::thread::sleep(Duration::from_millis(100));
    assert!(calls(&fake, &speaker).is_empty());
}

#[gpui::test]
fn a_videos_sound_goes_to_a_speaker_and_the_picture_here_follows_it(cx: &mut TestAppContext) {
    let (window, mut cx, fake, _) = start(cx, "clip.mp4", false);
    let speaker = speaker(&fake, "Kitchen");
    let view = tile(window, &cx);
    let load = cast(window, &mut cx, &fake, &speaker);
    assert!(load.picture_here);
    assert!(!load.autoplay, "a paused tile stays paused there");
    assert_eq!(load.mime, "audio/aac");
    assert_eq!(load.subtitles, None);
    let (status, body) = get(&load.url);
    assert_eq!(status, 200);
    assert!(
        body.len() > 7 && body[0] == 0xFF && body[1] & 0xF0 == 0xF0,
        "the sound alone, as ADTS"
    );
    assert!(view.read_with(&cx, |view, _| view.player().is_sound_elsewhere()));

    // Playing, but not yet heard moving: the picture waits, where it is.
    say(&fake, &speaker, State::Playing, 0.5);
    wait(&mut cx, "the picture to be where the sound is", |cx| {
        view.read_with(cx, |view, _| (view.player().position() - 0.5).abs() < 1e-9)
    });
    assert!(!view.read_with(&cx, |view, _| view.player().is_playing()));
    assert!(
        !shows(&mut cx, "play-here"),
        "the picture stays on the tile, not a panel saying where it went"
    );

    // Heard moving, the picture plays with it.
    say(&fake, &speaker, State::Playing, 0.6);
    wait(&mut cx, "the picture to play with the sound", |cx| {
        view.read_with(cx, |view, _| view.player().is_playing())
    });
    // Paused there, as from its own buttons: the picture stops where it did.
    say(&fake, &speaker, State::Paused, 1.25);
    wait(&mut cx, "the picture to stop where the sound did", |cx| {
        view.read_with(cx, |view, _| {
            !view.player().is_playing() && (view.player().position() - 1.25).abs() < 1e-9
        })
    });

    // The tile's controls reach the speaker, and stop the picture at once.
    press(&mut cx, "space");
    wait(&mut cx, "the speaker to play", |_| {
        calls(&fake, &speaker).contains(&Call::Play)
    });
    say(&fake, &speaker, State::Playing, 1.3);
    wait(&mut cx, "the picture to play with the sound", |cx| {
        view.read_with(cx, |view, _| view.player().is_playing())
    });
    press(&mut cx, "space");
    assert!(!view.read_with(&cx, |view, _| view.player().is_playing()));
    wait(&mut cx, "the speaker to pause", |_| {
        calls(&fake, &speaker).contains(&Call::Pause)
    });
    say(&fake, &speaker, State::Paused, 1.5);
    wait(&mut cx, "the picture to be where the sound stopped", |cx| {
        view.read_with(cx, |view, _| (view.player().position() - 1.5).abs() < 1e-9)
    });

    view.update(&mut cx, |view, cx| view.cast_to(None, cx));
    let (casting, position, playing, elsewhere) = view.read_with(&cx, |view, _| {
        (
            view.casting().cloned(),
            view.player().position(),
            view.player().is_playing(),
            view.player().is_sound_elsewhere(),
        )
    });
    assert_eq!(casting, None);
    assert!((position - 1.5).abs() < 1e-9, "it stays at {position}");
    assert!(!playing, "paused, as the speaker was");
    assert!(!elsewhere, "the sound is back here");
    wait(&mut cx, "the speaker to stop", |_| {
        calls(&fake, &speaker).contains(&Call::Stop)
    });
}

#[gpui::test]
fn the_picture_here_shows_where_the_slider_is_dragged_and_the_speaker_goes_there(
    cx: &mut TestAppContext,
) {
    let (window, mut cx, fake, _) = start(cx, "clip.mp4", false);
    let speaker = speaker(&fake, "Kitchen");
    let view = tile(window, &cx);
    cast(window, &mut cx, &fake, &speaker);
    say(&fake, &speaker, State::Paused, 0.25);
    wait(&mut cx, "the picture to be where the sound is", |cx| {
        view.read_with(cx, |view, _| (view.player().position() - 0.25).abs() < 1e-9)
    });

    let bar = find(&mut cx, "seek");
    let at = |fraction: f32| point(bar.left() + bar.size.width * fraction, bar.center().y);
    cx.simulate_mouse_move(at(0.5), None, Modifiers::none());
    cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(at(0.8), MouseButton::Left, Modifiers::none());
    let shown = view.read_with(&cx, |view, _| view.player().position());
    assert!(shown > 1.2, "the picture shows {shown}");
    // However often the tile hears where the speaker is meanwhile.
    for _ in 0..10 {
        cx.executor().advance_clock(CAST_POLL);
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    let position = view.read_with(&cx, |view, _| view.player().position());
    assert_eq!(position, shown, "it stays where the slider is");
    assert!(
        !calls(&fake, &speaker)
            .iter()
            .any(|call| matches!(call, Call::Seek(_))),
        "the speaker waits for the slider to be let go"
    );

    cx.simulate_mouse_up(at(0.8), MouseButton::Left, Modifiers::none());
    wait(&mut cx, "the speaker to seek", |_| {
        calls(&fake, &speaker)
            .iter()
            .any(|call| matches!(call, Call::Seek(_)))
    });
    let sought = calls(&fake, &speaker)
        .into_iter()
        .find_map(|call| match call {
            Call::Seek(time) => Some(time),
            _ => None,
        });
    assert_eq!(sought, Some(shown), "where the picture showed");
}

#[gpui::test]
fn quitting_stops_the_device(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip.mp4", false);
    let view = tile(window, &cx);
    cast(window, &mut cx, &fake, &tv);
    say(&fake, &tv, State::Playing, 1.5);
    wait(&mut cx, "the bar to follow the device", |cx| {
        view.read_with(cx, |view, _| (view.position() - 1.5).abs() < 1e-9)
    });
    let stopping = window
        .update(&mut cx, |workspace, _, cx| workspace.stop_casts(cx))
        .unwrap();
    assert_eq!(stopping.len(), 1);
    for thread in stopping {
        thread.join().unwrap();
    }
    assert_eq!(calls(&fake, &tv).last(), Some(&Call::Stop));
    // Where to resume is where the device was, not where the cast began.
    let kept = window
        .update(&mut cx, |workspace, _, cx| workspace.progress(cx))
        .unwrap();
    let [(_, position, _)] = kept.as_slice() else {
        panic!("one file's progress, not {kept:?}");
    };
    assert!((position - 1.5).abs() < 0.05, "kept {position}");
}
