//! The commands a window's controls send, run as the API runs them: what
//! each does to the tile or window it's aimed at.

use std::slice;

use gpui::{
    Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, WindowHandle, point,
};
use serde_json::{Value, json};

use super::open;
use crate::api::{self, Error, Invocation};
use crate::media::tracks::TrackKind;
use crate::recents;
use crate::ui::player::PlayerView;
use crate::ui::testing::{self, find, fixture, hover, wait_until};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// Run `invocation` as a deferred command does, outside any window's
/// update, and what it hands back.
fn run(cx: &mut VisualTestContext, invocation: Invocation) -> Result<Value, Error> {
    let done = TestAppContext::update(cx, |cx| api::run(invocation, cx));
    cx.run_until_parked();
    done
}

/// Run `command`, given `args`, on `tile`.
fn run_on(
    cx: &mut VisualTestContext,
    tile: &Entity<PlayerView>,
    command: &str,
    args: Value,
) -> Result<Value, Error> {
    let name = tile.read_with(cx, |view, _| view.id().to_string());
    run(cx, Invocation::new(command).with_args(args).on_tile(name))
}

/// `window`'s tiles.
fn tiles(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<Entity<PlayerView>> {
    window
        .read_with(cx, |workspace, _| workspace.players())
        .unwrap()
}

/// The API's name for `window`.
fn name(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> String {
    window
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap()
}

/// The player window the API names `name`.
fn named(name: &str, cx: &VisualTestContext) -> WindowHandle<Workspace> {
    cx.windows()
        .into_iter()
        .filter_map(|window| window.downcast::<Workspace>())
        .find(|window| {
            window
                .read_with(cx, |workspace, _| workspace.id().to_string() == name)
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("no window {name}"))
}

#[gpui::test]
fn seeking_plays_from_the_time_given_no_later_than_the_end(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let tile = tiles(window, &cx)[0].clone();
    let duration = tile.read_with(&cx, |view, _| view.duration()).unwrap();
    let position = |cx: &VisualTestContext| tile.read_with(cx, |view, _| view.player().position());

    run_on(&mut cx, &tile, "playback.seek", json!({"time": 1.0})).unwrap();
    assert!((position(&cx) - 1.0).abs() < 1e-9, "at {}", position(&cx));
    run_on(&mut cx, &tile, "playback.seek", json!({"time": 1e6})).unwrap();
    assert!(
        position(&cx) <= duration,
        "no further than the end, {duration}: {}",
        position(&cx)
    );
    let before = run_on(&mut cx, &tile, "playback.seek", json!({"time": -1}));
    assert_eq!(
        before,
        Err(Error::bad_args("playback.seek", "time is at least 0"))
    );
}

#[gpui::test]
fn the_seek_bar_dragged_holds_and_let_go_seeks_and_plays_on(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], true);
    let tile = tiles(window, &cx)[0].clone();
    let duration = tile.read_with(&cx, |view, _| view.duration()).unwrap();
    let playing = |cx: &VisualTestContext| tile.read_with(cx, |view, _| view.player().is_playing());
    cx.update(|_, cx| api::record(cx));
    hover(&mut cx, "tile-0");
    let bar = find(&mut cx, "seek");
    let from = point(bar.left() + bar.size.width * 0.2, bar.center().y);
    let to = point(from.x + bar.size.width * 0.5, from.y);

    cx.simulate_mouse_move(from, None, Modifiers::none());
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert!(!playing(&cx), "it holds while dragged");
    assert_eq!(
        cx.update(|_, cx| api::recorded(cx)),
        [],
        "it follows here, sending nothing"
    );

    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    let sent = cx.update(|_, cx| api::recorded(cx));
    assert_eq!(sent.len(), 1, "one seek, once let go: {sent:?}");
    assert_eq!(sent[0].command, "playback.seek");
    let time = sent[0].args["time"].as_f64().unwrap();
    assert!((time / duration - 0.7).abs() < 0.1, "{time} of {duration}");
    let at = tile.read_with(&cx, |view, _| view.player().position());
    assert!((at - time).abs() < 0.5, "at {at}, sent {time}");
    assert!(playing(&cx), "it plays on, as it played before the drag");

    // Played from there, the bar follows playback again.
    run_on(&mut cx, &tile, "playback.seek", json!({"time": 0.25})).unwrap();
    let (bar, at) = tile.read_with(&cx, |view, _| (view.bar_position(), view.position()));
    assert!(
        (at - 0.25).abs() < 0.2 && (bar - at).abs() < 1e-9,
        "the bar at {bar}, playing at {at}"
    );
}

#[gpui::test]
fn set_volume_sets_it_in_percent_and_unmutes_unless_none(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let tile = tiles(window, &cx)[0].clone();
    let sound = |cx: &VisualTestContext| {
        tile.read_with(cx, |view, _| {
            (view.player().volume(), view.player().is_muted())
        })
    };
    tile.update(&mut cx, |view, _| view.set_muted(true));

    run_on(&mut cx, &tile, "audio.set_volume", json!({"volume": 0})).unwrap();
    assert_eq!(sound(&cx), (0.0, true), "none leaves it muted");
    run_on(&mut cx, &tile, "audio.set_volume", json!({"volume": 40})).unwrap();
    assert_eq!(sound(&cx), (0.4, false));
    let loud = run_on(&mut cx, &tile, "audio.set_volume", json!({"volume": 101}));
    assert_eq!(
        loud,
        Err(Error::bad_args("audio.set_volume", "volume is at most 100"))
    );
}

#[gpui::test]
fn play_file_instead_plays_another_in_the_tile_as_this_one_was(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let tile = tiles(window, &cx)[0].clone();
    tile.update(&mut cx, |view, cx| {
        view.set_muted(true);
        view.toggle_loop(cx);
    });
    run_on(&mut cx, &tile, "audio.set_volume", json!({"volume": 0})).unwrap();
    let opus = fixture("clip-opus.mkv");

    run_on(&mut cx, &tile, "playback.play_file", json!({"path": opus})).unwrap();
    assert_eq!(
        tiles(window, &cx),
        slice::from_ref(&tile),
        "in the same tile"
    );
    let playing = wait_until(&mut cx, |cx| {
        tile.read_with(cx, |view, _| {
            view.file() == Some(opus.as_path()) && view.player().is_known()
        })
    });
    assert!(playing, "it plays the file given");
    tile.read_with(&cx, |view, _| {
        let player = view.player();
        assert_eq!(player.volume(), 0.0);
        assert!(player.is_muted() && player.is_looping(), "as the other was");
    });
    let noted = cx.update(|_, cx| recents::FILES.list(cx));
    assert_eq!(noted[0], opus.to_string_lossy(), "played lately, first");

    let relative = run_on(
        &mut cx,
        &tile,
        "playback.play_file",
        json!({"path": "clip.mp4"}),
    );
    assert_eq!(
        relative,
        Err(Error::bad_args(
            "playback.play_file",
            "clip.mp4: give a file's full path"
        ))
    );
}

#[gpui::test]
fn tracks_are_picked_by_number_and_subtitles_by_none_too(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip-subs.mkv"], false);
    let tile = tiles(window, &cx)[0].clone();
    let selected = |cx: &VisualTestContext, kind| {
        tile.read_with(cx, |view, _| {
            view.player().info().and_then(|info| info.selected(kind))
        })
    };
    // The decoder says what plays once it has switched: of two picked one
    // after the other, the first may show for a moment.
    let settles = |cx: &mut VisualTestContext, kind, track: Option<usize>| {
        wait_until(cx, |cx| {
            tile.read_with(cx, |view, _| {
                view.player().info().and_then(|info| info.selected(kind))
            }) == track
        })
    };

    run_on(
        &mut cx,
        &tile,
        "subtitles.select_track",
        json!({"number": 1}),
    )
    .unwrap();
    assert_eq!(
        selected(&cx, TrackKind::Subtitle),
        Some(2),
        "stream 2, its first"
    );
    run_on(&mut cx, &tile, "subtitles.select_track", Value::Null).unwrap();
    assert!(settles(&mut cx, TrackKind::Subtitle, None), "none, none");
    run_on(
        &mut cx,
        &tile,
        "subtitles.select_track",
        json!({"number": 1}),
    )
    .unwrap();
    run_on(
        &mut cx,
        &tile,
        "subtitles.select_track",
        json!({"number": null}),
    )
    .unwrap();
    assert!(settles(&mut cx, TrackKind::Subtitle, None), "null, none");

    run_on(&mut cx, &tile, "audio.select_track", json!({"number": 1})).unwrap();
    assert_eq!(selected(&cx, TrackKind::Audio), Some(1));
    for (command, number) in [
        ("audio.select_track", 2),
        ("video.select_track", 2),
        ("subtitles.select_track", 2),
    ] {
        let missing = run_on(&mut cx, &tile, command, json!({"number": number}));
        assert_eq!(missing, Err(Error::not_found("track", "2")), "{command}");
    }
    let none = run_on(&mut cx, &tile, "audio.select_track", Value::Null);
    assert!(
        matches!(none, Err(Error::BadArgs { .. })),
        "audio needs a number: {none:?}"
    );
}

#[gpui::test]
fn a_tile_is_maximized_and_moved_about_its_grid(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(
        window,
        &mut cx,
        &["clip.mp4", "clip-ip.mkv", "tone.m4a"],
        false,
    );
    let [a, b, c] = &tiles(window, &cx)[..] else {
        panic!("three tiles");
    };
    let maximized = |cx: &VisualTestContext| {
        window
            .read_with(cx, |workspace, _| workspace.maximized)
            .unwrap()
    };

    run_on(&mut cx, b, "tile.zoom", Value::Null).unwrap();
    assert_eq!(maximized(&cx), Some(b.entity_id()));
    run_on(&mut cx, b, "tile.zoom", Value::Null).unwrap();
    assert_eq!(maximized(&cx), None, "and back");
    assert!(
        !cx.update(|window, _| window.is_fullscreen()),
        "the window stays as it is among others"
    );

    run_on(&mut cx, a, "tile.move", json!({"position": 3})).unwrap();
    assert_eq!(tiles(window, &cx), [b.clone(), c.clone(), a.clone()]);
    run_on(&mut cx, a, "tile.move", json!({"position": 1})).unwrap();
    assert_eq!(tiles(window, &cx), [a.clone(), b.clone(), c.clone()]);
    let past = run_on(&mut cx, a, "tile.move", json!({"position": 4}));
    assert_eq!(
        past,
        Err(Error::bad_args(
            "tile.move",
            "position is at most 3, as many as the window has tiles"
        ))
    );

    // Alone, it fills the screen.
    for tile in [b, c] {
        run_on(&mut cx, tile, "tile.close", Value::Null).unwrap();
    }
    run_on(&mut cx, a, "tile.zoom", Value::Null).unwrap();
    assert!(cx.update(|window, _| window.is_fullscreen()));
    assert_eq!(maximized(&cx), None);
}

#[gpui::test]
fn a_tile_moves_to_another_window_or_one_of_its_own_and_plays_on(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    open(first, &mut cx, &["clip.mp4", "tone.m4a"], true);
    let [a, b] = &tiles(first, &cx)[..] else {
        panic!("two tiles");
    };
    let second = cx
        .update(|_, cx| Workspace::open_window(WINDOW_SIZE, cx))
        .unwrap();
    let mut second_cx = VisualTestContext::from_window(second.into(), &cx);
    open(second, &mut second_cx, &["clip-ip.mkv"], false);
    let c = tiles(second, &cx)[0].clone();
    let playing = |tile: &Entity<PlayerView>, cx: &VisualTestContext| {
        tile.read_with(cx, |view, _| view.player().is_playing())
    };
    let in_front = |cx: &VisualTestContext| {
        second
            .read_with(cx, |workspace, _| workspace.active_view())
            .unwrap()
    };

    let to = json!({"window": name(second, &cx)});
    run_on(&mut cx, a, "tile.move_to_window", to.clone()).unwrap();
    assert_eq!(tiles(first, &cx), slice::from_ref(b));
    assert_eq!(tiles(second, &cx), [c.clone(), a.clone()], "last there");
    assert!(playing(a, &cx), "it plays on");
    assert_eq!(in_front(&cx), Some(a.clone()), "keys go to it there");
    run_on(&mut cx, &c, "tile.move_to_window", to).unwrap();
    assert_eq!(
        tiles(second, &cx),
        [c.clone(), a.clone()],
        "one there already stays where it is"
    );
    let nowhere = run_on(
        &mut cx,
        a,
        "tile.move_to_window",
        json!({"window": "nowhere"}),
    );
    assert!(
        matches!(nowhere, Err(ref err) if err.is_not_found()),
        "{nowhere:?}"
    );

    let opened = run_on(&mut cx, b, "tile.move_to_new_window", Value::Null).unwrap();
    let own = named(opened["window"].as_str().expect("its window"), &cx);
    assert_ne!(own, first);
    assert_ne!(own, second);
    assert_eq!(tiles(own, &cx), slice::from_ref(b));
    assert!(tiles(first, &cx).is_empty());
    assert!(playing(b, &cx), "it plays on");
}

#[gpui::test]
fn open_plays_paths_and_urls_in_the_window_named_or_in_front(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    let second = cx
        .update(|_, cx| Workspace::open_window(WINDOW_SIZE, cx))
        .unwrap();
    cx.run_until_parked();
    let clip = fixture("clip.mp4");
    let path = clip.display().to_string();
    let url = format!("file://{path}");

    let there = name(second, &cx);
    let open = Invocation::new("app.open")
        .with_args(json!({"urls": [path, url]}))
        .in_window(there.clone());
    let opened = run(&mut cx, open).unwrap();
    assert_eq!(opened, json!({"window": there}));
    let both = wait_until(&mut cx, |cx| {
        second
            .read_with(cx, |workspace, _| workspace.players().len() == 2)
            .unwrap()
    });
    assert!(both, "a path and a file:// URL open in the window named");
    assert!(tiles(first, &cx).is_empty());

    for (url, why) in [
        ("clip.mp4", "clip.mp4: give a file's full path, or a URL"),
        (
            "gopher://192.0.2.1/clip.mp4",
            "gopher://192.0.2.1/clip.mp4: gopher:// streams aren't supported",
        ),
    ] {
        let wrong = run(
            &mut cx,
            Invocation::new("app.open").with_args(json!({"urls": [url]})),
        );
        assert_eq!(wrong, Err(Error::bad_args("app.open", why)));
    }
    let none = run(
        &mut cx,
        Invocation::new("app.open").with_args(json!({"urls": []})),
    );
    assert!(matches!(none, Err(Error::BadArgs { .. })), "{none:?}");
}

#[gpui::test]
fn open_with_no_window_open_opens_one(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    window
        .update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    let path = fixture("clip.mp4").display().to_string();

    let opened = run(
        &mut cx,
        Invocation::new("app.open").with_args(json!({"urls": [path]})),
    )
    .unwrap();
    let new = named(opened["window"].as_str().expect("its window"), &cx);
    assert_ne!(new, window);
    let open = wait_until(&mut cx, |cx| {
        new.read_with(cx, |workspace, _| workspace.players().len() == 1)
            .unwrap()
    });
    assert!(open, "the file opens in it");
}
