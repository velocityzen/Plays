//! The commands that list and inspect windows, blocks and devices, as the
//! command line reads them: what each hands back, shape and all.

use std::sync::Arc;

use gpui::{Bounds, TestAppContext, VisualTestContext, WindowHandle, point, px, size};
use serde_json::{Value, json};

use super::open;
use crate::api::{self, Invocation};
use crate::app::AudioEngine;
use crate::cast::fake::FakeTarget;
use crate::cast::{Casting, Shows};
use crate::files;
use crate::media::Source;
use crate::media::output::DeviceInfo;
use crate::ui::displays::{self, DisplayInfo};
use crate::ui::testing::{self, fixture, wait_until};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// What `invocation` hands back, run as a deferred command is.
fn query(cx: &mut VisualTestContext, invocation: Invocation) -> Value {
    let done = TestAppContext::update(cx, |cx| api::run(invocation, cx));
    cx.run_until_parked();
    done.unwrap_or_else(|err| panic!("{err:?}"))
}

/// Run `command`, given `args`, on the block `name` names.
fn on_block(cx: &mut VisualTestContext, command: &str, args: Value, name: &str) -> Value {
    query(cx, Invocation::new(command).with_args(args).on_block(name))
}

fn id(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> String {
    window
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap()
}

/// The ids of `window`'s blocks, in order.
fn block_ids(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> Vec<String> {
    window
        .read_with(cx, |workspace, cx| {
            workspace
                .players()
                .iter()
                .map(|block| block.read(cx).id().to_string())
                .collect()
        })
        .unwrap()
}

fn path(file: &str) -> String {
    fixture(file).display().to_string()
}

#[gpui::test]
fn window_ls_and_inspect_say_what_each_window_is(cx: &mut TestAppContext) {
    let (first, mut cx) = testing::start(cx);
    open(first, &mut cx, &["clip.mp4", "tone.m4a"], false);
    let second = TestAppContext::update(&cx, |cx| Workspace::open_window(WINDOW_SIZE, cx)).unwrap();
    cx.run_until_parked();
    let (one, two) = (id(first, &cx), id(second, &cx));
    let rename = Invocation::new("window.rename")
        .with_args(json!({"label": "Stage"}))
        .in_window(two.as_str());
    query(&mut cx, rename);

    assert_eq!(
        query(&mut cx, Invocation::new("window.ls")),
        json!([
            {"id": one, "label": null, "title": "Plays", "number": 1, "front": true, "blocks": 2},
            {"id": two, "label": "Stage", "title": "Stage", "number": 2, "front": false, "blocks": 0},
        ])
    );

    let bounds = cx.update(|window, _| window.bounds());
    let display = cx.update(|window, cx| displays::of(window, cx));
    let blocks = query(&mut cx, Invocation::new("block.ls"));
    assert_eq!(
        query(&mut cx, Invocation::new("window.inspect")),
        json!({
            "id": one,
            "label": null,
            "title": "Plays",
            "number": 1,
            "front": true,
            "blocks": blocks,
            "bounds": {
                "x": f32::from(bounds.origin.x),
                "y": f32::from(bounds.origin.y),
                "width": f32::from(bounds.size.width),
                "height": f32::from(bounds.size.height),
            },
            "display": display,
            "full_screen": false,
            "on_top": false,
            "visible": ["controls", "top_bar", "track_switcher", "notes"],
        })
    );
    let other = query(
        &mut cx,
        Invocation::new("window.inspect").in_window("Stage"),
    );
    assert_eq!(
        (&other["number"], &other["front"], &other["blocks"]),
        (&json!(2), &json!(false), &json!([])),
        "named"
    );

    // Full screen and floating on top show.
    cx.update(|window, _| window.toggle_fullscreen());
    query(&mut cx, Invocation::new("window.float_on_top.toggle"));
    let inspected = query(&mut cx, Invocation::new("window.inspect"));
    assert_eq!(inspected["full_screen"], true);
    assert_eq!(inspected["on_top"], true);
}

/// A window with a file with chapters and one with subtitles, and the
/// ids of their blocks.
struct TwoFiles {
    window: WindowHandle<Workspace>,
    cx: VisualTestContext,
    chapters: String,
    subs: String,
}

fn two_files(cx: &mut TestAppContext) -> TwoFiles {
    let (window, mut cx) = testing::start(cx);
    open(
        window,
        &mut cx,
        &["clip-chapters.mkv", "clip-subs.mkv"],
        false,
    );
    let [chapters, subs] = <[String; 2]>::try_from(block_ids(window, &cx)).expect("two blocks");
    TwoFiles {
        window,
        cx,
        chapters,
        subs,
    }
}

#[gpui::test]
fn block_ls_says_what_each_block_is(cx: &mut TestAppContext) {
    let TwoFiles {
        mut cx,
        chapters,
        subs,
        ..
    } = two_files(cx);
    query(&mut cx, Invocation::new("app.playback.play"));
    let listed = query(&mut cx, Invocation::new("block.ls"));
    assert_eq!(listed[0]["playing"], true);
    query(&mut cx, Invocation::new("app.playback.pause"));
    let listed = query(&mut cx, Invocation::new("block.ls"));
    assert_eq!(
        listed,
        json!([
            {
                "id": chapters,
                "label": null,
                "title": "clip-chapters.mkv",
                "file": path("clip-chapters.mkv"),
                "url": null,
                "playing": false,
                "opened": true,
                "error": null,
                "number": 1,
                "front": true,
            },
            {
                "id": subs,
                "label": null,
                "title": "clip-subs.mkv",
                "file": path("clip-subs.mkv"),
                "url": null,
                "playing": false,
                "opened": true,
                "error": null,
                "number": 2,
                "front": false,
            },
        ])
    );
}

#[gpui::test]
fn block_ls_says_whether_a_blocks_file_opened_is_opening_or_couldnt(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let folder = files::test_folder("inspect-opening");
    std::fs::create_dir_all(&folder).unwrap();
    let broken = folder.join("broken.mp4");
    std::fs::write(&broken, b"not a video, nor anything else").unwrap();
    // Opening it waits for something to write to it.
    let waiting = folder.join("waiting.mp4");
    let made = std::process::Command::new("mkfifo")
        .arg(&waiting)
        .status()
        .unwrap();
    assert!(made.success());
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![fixture("clip.mp4"), broken, waiting.clone()], cx);
        })
        .unwrap();
    let state = |cx: &mut TestAppContext| -> Vec<(Value, Value)> {
        let listed = cx.update(|cx| api::run(Invocation::new("block.ls"), cx));
        listed
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|block| (block["opened"].clone(), block["error"].clone()))
            .collect()
    };
    let settled = wait_until(&mut cx, |cx| {
        let state = state(cx);
        state.len() == 3 && state[0].0 == true && !state[1].1.is_null()
    });
    let listed = state(&mut cx);
    assert!(settled, "{listed:?}");
    assert_eq!(listed[2], (json!(false), Value::Null), "still opening");
    let ids = block_ids(window, &cx);
    let opening = Invocation::new("block.inspect").on_block(ids[2].clone());
    let inspected = TestAppContext::update(&cx, |cx| api::run(opening, cx)).unwrap();
    assert_eq!(
        inspected["buffering"], true,
        "waiting for what it plays from"
    );
    assert!(
        listed[1].0 == false && listed[1].1.as_str().is_some_and(|why| !why.is_empty()),
        "couldn't, and why: {listed:?}"
    );
    assert_eq!(listed[0], (json!(true), Value::Null));

    // Nothing written: it can't open either.
    drop(std::fs::File::create(&waiting).unwrap());
    let failed = wait_until(&mut cx, |cx| !state(cx)[2].1.is_null());
    assert!(failed, "{:?}", state(&mut cx));
}

/// Tick `window`'s blocks, as drawing does, until none is buffering: a
/// seek has its frame. Whether that was in time.
fn buffered(window: WindowHandle<Workspace>, cx: &mut TestAppContext) -> bool {
    wait_until(cx, |cx| {
        window
            .update(cx, |workspace, _, cx| {
                workspace.players().iter().all(|view| {
                    view.update(cx, |view, cx| {
                        view.background_tick(cx);
                        !view.player().is_buffering()
                    })
                })
            })
            .unwrap()
    })
}

#[gpui::test]
fn block_inspect_says_how_a_block_plays(cx: &mut TestAppContext) {
    let TwoFiles {
        window,
        mut cx,
        subs,
        ..
    } = two_files(cx);
    query(&mut cx, Invocation::new("app.playback.pause"));
    for (command, args) in [
        ("block.rename", json!({"label": "Subs"})),
        ("block.playback.seek", json!({"time": 1.0})),
        ("block.playback.loop.toggle", Value::Null),
        ("block.playback.set_speed", json!({"speed": 1.5})),
        ("block.audio.set_volume", json!({"volume": 50})),
        ("block.audio.mute.toggle", Value::Null),
        ("block.audio.later", Value::Null),
        ("block.subtitles.earlier", Value::Null),
        ("block.subtitles.select_track", json!({"number": 1})),
        // Mirrored, then turned: as kept.
        ("block.video.flip_horizontally", Value::Null),
        ("block.video.rotate_right", Value::Null),
        ("block.video.set_aspect_ratio", json!({"ratio": "4:3"})),
        ("block.video.set_crop", json!({"crop": "16:9"})),
    ] {
        on_block(&mut cx, command, args, &subs);
    }
    assert!(buffered(window, &mut cx), "the seek's frame didn't come");
    let track = |codec: &str, format: Option<&str>| {
        json!({
            "number": 1,
            "label": "Track 1",
            "language": null,
            "title": null,
            "codec": codec,
            "format": format,
            "default": true,
            "forced": false,
            "hearing_impaired": false,
            "file": null,
        })
    };
    let expected = json!({
        "id": subs,
        "label": "Subs",
        "title": "clip-subs.mkv",
        "file": path("clip-subs.mkv"),
        "url": null,
        "playing": false,
        "opened": true, "error": null,
        "number": 2,
        "front": false,
        "window": id(window, &cx),
        "window_title": "Plays",
        "position": 1.0,
        "duration": 2.021,
        "frame_rate": 25.0,
        "ended": false, "buffering": false,
        "volume": 50.0, "muted": true, "speed": 1.5, "looping": true, "behind_live": null,
        "tracks": {
            "video": [track("h264", Some("160×120"))],
            "audio": [track("aac", Some("Mono"))],
            "subtitles": [{
                "number": 1,
                "label": "English",
                "language": "eng",
                "title": "English",
                "codec": "subrip",
                "format": null,
                "default": false,
                "forced": false,
                "hearing_impaired": false,
                "file": null,
            }],
        },
        "picked": {"video": 1, "audio": 1, "subtitles": 1},
        "delays": {"audio": 0.05, "subtitles": -0.1},
        "picture": {"turns": 1, "mirrored": true, "aspect_ratio": "4:3", "crop": "16:9"},
        "chapter": null, "chapters": [],
        "casting": null, "output": null,
        "visible": ["controls", "top_bar", "track_switcher", "notes"],
    });
    assert_eq!(
        on_block(&mut cx, "block.inspect", Value::Null, "Subs"),
        expected
    );

    // Its own output, once picked.
    cx.update(|_, cx| {
        let desk = DeviceInfo {
            id: "desk".into(),
            name: "Desk".into(),
            bluetooth: false,
        };
        AudioEngine::set_devices(vec![desk], cx);
    });
    on_block(
        &mut cx,
        "output.set_block_device",
        json!({"id": "desk"}),
        "Subs",
    );
    let inspected = on_block(&mut cx, "block.inspect", Value::Null, "Subs");
    assert_eq!(inspected["output"], json!({"id": "desk", "name": "Desk"}));
}

#[gpui::test]
fn block_inspect_says_its_chapters_and_its_end(cx: &mut TestAppContext) {
    let TwoFiles {
        mut cx, chapters, ..
    } = two_files(cx);
    // The chapters, and the one it's in, from 1.
    on_block(
        &mut cx,
        "block.playback.go_to_chapter",
        json!({"number": 2}),
        &chapters,
    );
    let inspected = on_block(&mut cx, "block.inspect", Value::Null, &chapters);
    assert_eq!(
        (&inspected["chapter"], &inspected["chapters"]),
        (
            &json!(2),
            &json!([
                {"number": 1, "name": "Opening", "start": 0.0, "end": 0.8},
                {"number": 2, "name": "Middle", "start": 0.8, "end": 1.4},
                {"number": 3, "name": "End", "start": 1.4, "end": 2.0},
            ])
        )
    );
    assert_eq!(
        inspected["picked"]["subtitles"],
        Value::Null,
        "none to pick"
    );

    // At its end, it says so.
    on_block(
        &mut cx,
        "block.playback.seek",
        json!({"time": 3.0}),
        &chapters,
    );
    let inspected = on_block(&mut cx, "block.inspect", Value::Null, &chapters);
    assert_eq!(
        (&inspected["position"], &inspected["ended"]),
        (&json!(2.021), &json!(true))
    );
}

#[gpui::test]
fn output_display_and_cast_ls_list_whats_there(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let display = |id: u32, name: &str, x: f32| DisplayInfo {
        id,
        name: name.into(),
        bounds: Bounds::new(point(px(x), px(0.)), size(px(1920.), px(1080.))),
    };
    let fake = FakeTarget::default();
    let tv = fake.add("Living Room", Shows::Video);
    fake.add("Kitchen", Shows::Sound);
    cx.update(|_, cx| {
        let device = |id: &str, name: &str| DeviceInfo {
            id: id.into(),
            name: name.into(),
            bluetooth: false,
        };
        let outputs = vec![
            device("desk", "Desk"),
            device("airplay:00-00-5e-00-53-01", "Den"),
        ];
        AudioEngine::set_devices(outputs, cx);
        displays::remember(
            vec![display(1, "Studio", 0.), display(2, "Side", 1920.)],
            cx,
        );
        cx.set_global(Casting::local(vec![Arc::new(fake.clone())]));
    });

    let output = query(&mut cx, Invocation::new("output.ls"));
    assert_eq!(
        output,
        json!({
            "outputs": [
                {"id": "desk", "name": "Desk", "kind": null},
                {"id": "airplay:00-00-5e-00-53-01", "name": "Den", "kind": "AirPlay"},
            ],
            "device": null,
            "route": {"unavailable": "Silent in tests"},
        })
    );
    let pick = Invocation::new("output.set_device").with_args(json!({"id": "desk"}));
    query(&mut cx, pick);
    let output = query(&mut cx, Invocation::new("output.ls"));
    assert_eq!(
        output["device"],
        json!({"id": "desk", "name": "Desk"}),
        "picked"
    );

    assert_eq!(
        query(&mut cx, Invocation::new("display.ls")),
        json!([
            {"id": 1, "name": "Studio", "bounds": {"x": 0.0, "y": 0.0, "width": 1920.0, "height": 1080.0}},
            {"id": 2, "name": "Side", "bounds": {"x": 1920.0, "y": 0.0, "width": 1920.0, "height": 1080.0}},
        ])
    );

    let found = wait_until(&mut cx, |cx| {
        TestAppContext::read(cx, |cx| Casting::devices(cx).len() == 2)
    });
    assert!(found, "the fake devices weren't found");
    assert_eq!(
        query(&mut cx, Invocation::new("cast.ls")),
        json!({"Fake": [
            {"id": "fake-1", "name": "Kitchen", "shows": "sound"},
            {"id": "fake-0", "name": "Living Room", "shows": "video"},
        ]}),
        "by protocol, then name"
    );

    // What a block casts to, it says.
    open(window, &mut cx, &["clip.mp4"], false);
    let block = Invocation::new("cast.to")
        .with_args(json!({"device": tv.id}))
        .on_block("1");
    query(&mut cx, block);
    let inspected = on_block(&mut cx, "block.inspect", Value::Null, "1");
    assert_eq!(
        inspected["casting"],
        json!({"id": "fake-0", "kind": "Fake", "name": "Living Room"})
    );
    assert_eq!(
        inspected["tracks"]["subtitles"][0]["file"],
        json!(path("clip.srt")),
        "subtitles from a file beside it"
    );

    // Plays' version, its windows and its output, together.
    let windows = query(&mut cx, Invocation::new("window.ls"));
    let output = query(&mut cx, Invocation::new("output.ls"));
    assert_eq!(
        query(&mut cx, Invocation::new("app.inspect")),
        json!({"version": env!("CARGO_PKG_VERSION"), "windows": windows, "output": output})
    );
}

#[gpui::test]
fn app_formats_says_what_plays_opens_each_video_or_audio(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    let formats = query(&mut cx, Invocation::new("app.formats"));
    let formats = formats.as_array().expect("a list");
    assert_eq!(formats.len(), file_types::FORMATS.len());
    let with = |extension: &str| {
        formats.iter().find(|format| {
            format["extensions"]
                .as_array()
                .unwrap()
                .contains(&json!(extension))
        })
    };
    let mkv = with("mkv").expect("mkv");
    assert_eq!(mkv["kind"], "video");
    assert!(
        mkv["name"].as_str().is_some_and(|name| !name.is_empty()),
        "{mkv}"
    );
    assert_eq!(with("flac").expect("flac")["kind"], "audio");
    assert!(with("srt").is_none(), "subtitles aren't opened alone");
}

#[gpui::test]
fn a_query_between_doesnt_end_a_walk_back(cx: &mut TestAppContext) {
    let (a, mut cx) = testing::start(cx);
    let others: Vec<WindowHandle<Workspace>> = (0..2)
        .map(|_| TestAppContext::update(&cx, |cx| Workspace::open_window(WINDOW_SIZE, cx)).unwrap())
        .collect();
    for window in [others[0], others[1]] {
        let mut window = VisualTestContext::from_window(window.into(), &cx);
        window.update(|window, _| window.activate_window());
        window.run_until_parked();
    }
    let front = |cx: &VisualTestContext| {
        TestAppContext::read(cx, |cx| cx.active_window().map(|window| window.window_id()))
    };
    query(&mut cx, Invocation::new("window.back"));
    assert_eq!(front(&cx), Some(others[0].window_id()));
    query(&mut cx, Invocation::new("window.ls"));
    query(&mut cx, Invocation::new("window.back"));
    assert_eq!(front(&cx), Some(a.window_id()), "on back, past it");
}

#[gpui::test]
fn a_streams_url_shows_without_its_user_password_or_query(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let url = "http://viewer:secret@192.0.2.10:8080/live/clip.mp4?token=abc";
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_sources(vec![Source::Url(url.into())], cx);
        })
        .unwrap();
    let opened = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, _| workspace.block_count() == 1)
            .unwrap()
    });
    assert!(opened, "no block for the stream");
    let listed = query(&mut cx, Invocation::new("block.ls"));
    assert_eq!(
        (&listed[0]["file"], &listed[0]["url"]),
        (&Value::Null, &json!("http://192.0.2.10:8080/live/clip.mp4"))
    );
}
