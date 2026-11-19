//! Casting a tile to a fake device, as someone at the screen would. The
//! stream server is the real one, serving this computer only, so what it
//! tells the device to fetch is fetched.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle};

use super::open;
use crate::cast::fake::{Call, FakeTarget};
use crate::cast::target::{Load, State};
use crate::cast::{Casting, Device, Shows};
use crate::media::tracks::TrackKind;
use crate::ui::player::PlayerView;
use crate::ui::testing::{self, OPEN_WAIT, click, press, shows};
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

fn tile(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Entity<PlayerView> {
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
fn wait(
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

/// What `url` serves: its status and body.
fn get(url: &str) -> (u16, Vec<u8>) {
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
    wait(&mut cx, "the stream server to let the file go", |_| {
        get(&load.url).0 == 404
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
fn speakers_are_offered_for_sound_only(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "tone.m4a", false);
    let speaker = fake.add("Kitchen", Shows::Sound);
    cx.update(|_, cx| Casting::refresh(cx));
    let offered = |cx: &mut VisualTestContext, has_video| {
        cx.update(|_, cx| {
            Casting::devices_for(cx, has_video)
                .into_iter()
                .map(|device| device.name)
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(offered(&mut cx, true), ["Living Room"]);
    assert_eq!(offered(&mut cx, false), ["Kitchen", "Living Room"]);

    let load = cast(window, &mut cx, &fake, &speaker);
    assert_eq!(load.mime, "audio/mp4");
    assert!(loads(&fake, &tv).is_empty());
}

#[gpui::test]
fn a_video_doesnt_go_to_a_speaker(cx: &mut TestAppContext) {
    let (window, mut cx, fake, _) = start(cx, "clip.mp4", false);
    let speaker = fake.add("Kitchen", Shows::Sound);
    let view = tile(window, &cx);
    view.update(&mut cx, |view, cx| view.cast_to(Some(speaker.clone()), cx));
    assert_eq!(view.read_with(&cx, |view, _| view.casting().cloned()), None);
    std::thread::sleep(Duration::from_millis(100));
    assert!(calls(&fake, &speaker).is_empty());
}

#[gpui::test]
fn quitting_stops_the_device(cx: &mut TestAppContext) {
    let (window, mut cx, fake, tv) = start(cx, "clip.mp4", false);
    cast(window, &mut cx, &fake, &tv);
    let stopping = window
        .update(&mut cx, |workspace, _, cx| workspace.stop_casts(cx))
        .unwrap();
    assert_eq!(stopping.len(), 1);
    for thread in stopping {
        thread.join().unwrap();
    }
    assert_eq!(calls(&fake, &tv).last(), Some(&Call::Stop));
}
