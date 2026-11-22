//! Each command tells the events it says it does (`Spec::tells`), and no
//! others: run in a world of its own, in each of the ways it's shown in,
//! while listening to everything.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{App, TestAppContext, VisualTestContext, WindowHandle};
use serde_json::json;

use super::*;
use crate::api::events::{self, Event, Scope as Heard};
use crate::app::AudioEngine;
use crate::cast::fake::FakeTarget;
use crate::cast::{Casting, Shows};
use crate::files::Folder;
use crate::media::output::DeviceInfo;
use crate::settings::Settings;
use crate::ui::testing::{self, fixture};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// Commands not run here, and why. They say they tell nothing.
const NOT_RUN: [(&str, &str); 2] = [
    (
        "window.open_files",
        "it asks for files, as the test platform can't",
    ),
    ("cli.install", "it links plays into /usr/local/bin"),
];

/// Events commands tell that can't be heard here, and why.
const NOT_HEARD: [(&str, &str, &str); 4] = [
    (
        "tile.toggle_picture_in_picture",
        "picture_in_picture_changed",
        "Picture in Picture is the system's",
    ),
    (
        "video.next_track",
        "track_changed",
        "no file here has two video tracks",
    ),
    (
        "video.select_track",
        "track_changed",
        "no file here has two video tracks",
    ),
    (
        "output.set_device",
        "route_changed",
        "tests play no sound, so it goes nowhere",
    ),
];

/// How long nothing new is told before a command is taken to be done.
const QUIET: Duration = Duration::from_millis(300);

/// What each command runs in: two player windows, the front one, in
/// front, with a file of several tracks (tile 1, keys going to it) and
/// one of a picture and sound (tile 2); the back one, first by number,
/// with sound alone.
struct World {
    back: String,
    device: String,
}

/// A step before listening: a command, or a change made directly.
enum Step {
    Run(Invocation),
    Do(fn(&mut App)),
}

/// A way to show what a command tells: from a world of its own, what's
/// done before listening, then the command's runs, while listening. The
/// first run must do what it does; another may find nothing left to.
struct Way {
    before: Vec<Step>,
    runs: Vec<Invocation>,
}

fn way(before: Vec<Invocation>, runs: Vec<Invocation>) -> Way {
    Way {
        before: before.into_iter().map(Step::Run).collect(),
        runs,
    }
}

fn path(name: &str) -> String {
    fixture(name).display().to_string()
}

/// Open `files` in the window in front, after those there.
fn open(files: &[&str]) -> Invocation {
    let urls: Vec<String> = files.iter().map(|file| path(file)).collect();
    Invocation::new("app.open").with_args(json!({"urls": urls}))
}

/// What `id` is run as, in `world`: the ways it's shown in are made of
/// these.
struct Plan<'a> {
    id: &'static str,
    world: &'a World,
}

impl Plan<'_> {
    /// It, aimed at what's in front.
    fn it(&self) -> Invocation {
        Invocation::new(self.id)
    }

    /// It, on the tile numbered `tile` in the window in front.
    fn on(&self, tile: &str) -> Invocation {
        self.it().on_tile(tile)
    }

    fn with(&self, args: serde_json::Value) -> Invocation {
        self.it().with_args(args)
    }

    /// It, in the back window.
    fn back(&self) -> Invocation {
        self.it().in_window(self.world.back.clone())
    }

    /// Run twice, from the world as it is.
    fn twice(invocation: Invocation) -> Vec<Way> {
        vec![way(Vec::new(), vec![invocation.clone(), invocation])]
    }
}

fn command(id: &str) -> Invocation {
    Invocation::new(id)
}

/// A file of chapters, third in the front window: Opening, Middle from
/// 0.8 s and End from 1.4 s.
fn chapters() -> Invocation {
    open(&["clip-chapters.mkv"])
}

/// Seek the tile keys go to to `time`.
fn at(time: f64) -> Invocation {
    command("playback.seek").with_args(json!({"time": time}))
}

/// Four tiles in the front window, two to a row.
fn four() -> Invocation {
    open(&["clip-ip.mkv", "clip-subs.mkv"])
}

/// The ways `spec` is shown in, in `world`: run twice from it as it is,
/// unless its namespace says otherwise.
fn ways(spec: &Spec, world: &World) -> Vec<Way> {
    let plan = Plan { id: spec.id, world };
    match spec.namespace() {
        "app" | "window" => window_ways(&plan),
        "tile" => tile_ways(&plan),
        "playback" => playback_ways(&plan),
        _ => other_ways(&plan),
    }
    .unwrap_or_else(|| Plan::twice(plan.it()))
}

fn window_ways(plan: &Plan) -> Option<Vec<Way>> {
    let ip = || plan.with(json!({"urls": [path("clip-ip.mkv")]}));
    Some(match plan.id {
        "app.open" => vec![
            way(Vec::new(), vec![ip()]),
            // With no window open, in a new one.
            way(
                vec![
                    command("window.close").in_window(plan.world.back.clone()),
                    command("window.close"),
                ],
                vec![ip()],
            ),
        ],
        "window.focus" => Plan::twice(plan.back()),
        "window.rename" => Plan::twice(plan.with(json!({"label": "Den"}))),
        "window.back" | "window.recent" => vec![way(vec![command("window.next")], vec![plan.it()])],
        "window.move" => Plan::twice(plan.with(json!({"position": 1}))),
        "window.exit_full_screen" => {
            vec![way(
                vec![command("window.toggle_full_screen")],
                vec![plan.it()],
            )]
        }
        "window.show_on_display" => Plan::twice(plan.with(json!({"display": 1}))),
        _ => return None,
    })
}

fn tile_ways(plan: &Plan) -> Option<Vec<Way>> {
    let third = || command("tile.focus").on_tile("3");
    Some(match plan.id {
        "tile.focus" => vec![
            way(Vec::new(), vec![plan.on("2")]),
            way(Vec::new(), vec![plan.back().on_tile("1")]),
        ],
        "tile.focus.left" | "tile.move.left" => {
            vec![way(vec![command("tile.focus.right")], vec![plan.it()])]
        }
        // Gone to the second tile, then back to the first.
        "tile.back" | "tile.recent" => vec![way(
            vec![command("tile.focus.right"), command("tile.focus.left")],
            vec![plan.it()],
        )],
        "tile.focus.up" | "tile.move.up" => vec![way(vec![four(), third()], vec![plan.it()])],
        "tile.focus.down" | "tile.move.down" => vec![way(vec![four()], vec![plan.it()])],
        "tile.zoom" => vec![
            way(Vec::new(), vec![plan.it(), plan.it()]),
            // A window's only tile fills the screen.
            way(Vec::new(), vec![plan.back()]),
        ],
        "tile.rename" => Plan::twice(plan.with(json!({"label": "Low"}))),
        "tile.move" => Plan::twice(plan.with(json!({"position": 2}))),
        "tile.move_to_window" => Plan::twice(plan.with(json!({"window": plan.world.back}))),
        _ => return None,
    })
}

fn playback_ways(plan: &Plan) -> Option<Vec<Way>> {
    let it = || plan.it();
    let third = || plan.on("3");
    let playing = || command("playback.play_pause");
    Some(match plan.id {
        "playback.seek" => vec![
            way(
                vec![chapters()],
                vec![plan.with(json!({"time": 1})).on_tile("3")],
            ),
            way(Vec::new(), vec![plan.with(json!({"time": 100}))]),
        ],
        "playback.seek_forward" | "playback.next_chapter" => {
            vec![way(vec![chapters()], vec![third(), third()])]
        }
        "playback.seek_forward_all" => vec![way(vec![chapters()], vec![it(), it()])],
        "playback.seek_backward" | "playback.restart" | "playback.previous_chapter" => {
            vec![way(vec![chapters(), at(1.5).on_tile("3")], vec![third()])]
        }
        "playback.seek_backward_all" | "playback.restart_all" => {
            vec![way(vec![chapters(), at(1.5).on_tile("3")], vec![it()])]
        }
        "playback.go_to_chapter" => vec![way(
            vec![chapters()],
            vec![plan.with(json!({"number": 2})).on_tile("3")],
        )],
        "playback.next_frame"
        | "playback.previous_frame"
        | "playback.next_frame_all"
        | "playback.previous_frame_all" => vec![way(vec![playing()], vec![it()])],
        "playback.pause_all" => vec![way(vec![command("playback.play_all")], vec![it()])],
        // Paused, and playing on, as it was.
        "playback.previous_file" | "playback.next_file" => {
            vec![
                way(Vec::new(), vec![it()]),
                way(vec![playing()], vec![it()]),
            ]
        }
        "playback.play_file" => {
            let file = || plan.with(json!({"path": path("clip-ip.mkv")}));
            vec![
                way(Vec::new(), vec![file()]),
                way(vec![playing()], vec![file()]),
            ]
        }
        // Where keys go, playing, or paused.
        "playback.sync_all" => vec![
            way(vec![playing()], vec![it()]),
            way(vec![command("playback.play_all"), playing()], vec![it()]),
        ],
        _ => return None,
    })
}

/// Keys for the play button, as Settings → Hotkeys sets them.
fn hotkeys() -> serde_json::Value {
    json!({"key": "hotkeys", "value": {"playback.play_pause": ["ctrl-p"]}})
}

fn other_ways(plan: &Plan) -> Option<Vec<Way>> {
    let device = || json!({"device": plan.world.device});
    Some(match plan.id {
        "audio.set_volume" => Plan::twice(plan.with(json!({"volume": 30}))),
        "audio.select_track" | "subtitles.select_track" => {
            Plan::twice(plan.with(json!({"number": 2})))
        }
        "video.select_track" => Plan::twice(plan.with(json!({"number": 1}))),
        "cast.to" | "cast.pair" => Plan::twice(plan.with(device())),
        "cast.stop" => vec![way(
            vec![command("cast.to").with_args(device())],
            vec![plan.it()],
        )],
        "output.set_device" | "output.set_window_device" => vec![
            way(Vec::new(), vec![plan.with(json!({"id": "speakers"}))]),
            // One not connected, waited for.
            way(
                Vec::new(),
                vec![plan.with(json!({"id": "headphones", "name": "Headphones"}))],
            ),
        ],
        "settings.set" => vec![
            way(
                Vec::new(),
                vec![plan.with(json!({"key": "autoplay", "value": true}))],
            ),
            way(Vec::new(), vec![plan.with(hotkeys())]),
        ],
        "settings.reset" => vec![
            way(Vec::new(), vec![plan.with(json!({"key": "autoplay"}))]),
            way(
                vec![command("settings.set").with_args(hotkeys())],
                vec![plan.with(json!({"key": "hotkeys"}))],
            ),
        ],
        // The file says otherwise than the settings Plays has.
        "settings.reload" => vec![Way {
            before: vec![Step::Do(|cx| {
                let kept = cx.global::<Settings>().clone();
                Settings::update(cx, true, |settings| {
                    settings.autoplay = !settings.autoplay;
                    settings.hotkeys.add("playback.play_pause", "ctrl-p");
                });
                Settings::update(cx, false, |settings| *settings = kept);
            })],
            runs: vec![plan.it()],
        }],
        _ => return None,
    })
}

/// Open `files` in a new player window, and wait for them to open.
fn new_window(files: &[&str], cx: &mut TestAppContext) -> WindowHandle<Workspace> {
    let window = cx
        .update(|cx| Workspace::open_window(WINDOW_SIZE, cx))
        .unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    super::open(window, files, &mut visual);
    window
}

/// A world of its own, every window there was closed, `settings` as they
/// were set out.
fn world(settings: &Settings, device: &str, cx: &mut TestAppContext) -> World {
    for window in cx.windows() {
        let mut visual = VisualTestContext::from_window(window, cx);
        visual.update(|window, _| window.remove_window());
        visual.run_until_parked();
    }
    cx.update(|cx| Settings::update(cx, false, |set| *set = settings.clone()));
    let back = new_window(&["tone.m4a"], cx);
    let front = new_window(&["clip-languages.mkv", "clip.mp4"], cx);
    let mut visual = VisualTestContext::from_window(front.into(), cx);
    visual.update(|window, _| window.activate_window());
    visual.run_until_parked();
    let back = back
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap();
    World {
        back,
        device: device.to_owned(),
    }
}

/// Whether every file in every window has opened, or failed to.
fn all_open(cx: &mut TestAppContext) -> bool {
    cx.update(|cx| {
        let windows = cx.windows();
        windows
            .into_iter()
            .filter_map(|window| window.downcast::<Workspace>())
            .filter_map(|window| window.read(cx).ok().map(Workspace::players))
            .flatten()
            .all(|player| {
                let player = player.read(cx).player();
                player.is_known() || player.error().is_some()
            })
    })
}

/// Run what's left to, wait for the files to open, and draw every
/// window, which the test platform does only when asked: a tile shows a
/// frame, to step from, only once drawn.
fn settle(cx: &mut TestAppContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(testing::wait_until(cx, all_open), "the files didn't open");
    for window in cx.windows() {
        let mut visual = VisualTestContext::from_window(window, cx);
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
    }
}

/// The types of the events `listener` has heard, but commands run.
fn told(listener: &mut UnboundedReceiver<Event>) -> impl Iterator<Item = &'static str> {
    std::iter::from_fn(|| listener.try_recv().ok())
        .map(|event| event.kind)
        .filter(|&kind| kind != "command_run")
}

/// The events `spec` tells, in each of its ways, each in a world of its
/// own; what went wrong running it, if anything.
fn hear(
    spec: &Spec,
    settings: &Settings,
    device: &str,
    cx: &mut TestAppContext,
) -> (BTreeSet<&'static str>, Vec<String>) {
    let declared: BTreeSet<&str> = spec.tells.iter().copied().collect();
    let mut heard = BTreeSet::new();
    let mut wrong = Vec::new();
    let unseen = World {
        back: String::new(),
        device: device.to_owned(),
    };
    let count = ways(spec, &unseen).len();
    for at in 0..count {
        let world = world(settings, device, cx);
        let way = ways(spec, &world).swap_remove(at);
        for step in way.before {
            match step {
                Step::Run(invocation) => {
                    let command = invocation.command.clone();
                    if let Err(err) = cx.update(|cx| run(invocation, cx)) {
                        wrong.push(format!("{}: {command}, before, failed: {err}", spec.id));
                    }
                }
                Step::Do(change) => cx.update(change),
            }
            settle(cx);
        }
        let mut listener = cx.update(|cx| events::listen(Heard::All, false, cx));
        for (run_at, invocation) in way.runs.into_iter().enumerate() {
            let done = cx.update(|cx| run(invocation, cx));
            if let (0, Err(err)) = (run_at, done) {
                wrong.push(format!("{}, way {}: {err}", spec.id, at + 1));
            }
            settle(cx);
        }
        // The test platform doesn't quit: what quitting tells is told as
        // the socket tells it as Plays quits.
        if spec.id == "app.quit" {
            cx.update(events::quit);
        }
        // Until all it says it tells is heard, or nothing more is.
        let mut last = Instant::now();
        loop {
            settle(cx);
            cx.update(events::look);
            let before = heard.len();
            heard.extend(told(&mut listener));
            if heard.len() > before {
                last = Instant::now();
            }
            if declared.is_subset(&heard) || last.elapsed() >= QUIET {
                break;
            }
        }
    }
    (heard, wrong)
}

/// Check that each command in `namespace` tells what it says it does.
fn check(namespace: &str, cx: &mut TestAppContext) {
    let _ = testing::start(cx);
    let screenshots =
        std::env::temp_dir().join(format!("plays-tells-{namespace}-{}", std::process::id()));
    std::fs::create_dir_all(&screenshots).unwrap();
    let fake = FakeTarget::default();
    let tv = fake.add("Living Room", Shows::Video);
    cx.update(|cx| {
        // Settings kept in a file, to reveal and read again.
        cx.set_global(Folder::for_test(&format!("tells-{namespace}")));
        Settings::update(cx, false, |settings| {
            settings.autoplay = false;
            // Each file starts at its start, with the tracks it says.
            settings.resume_files = false;
            settings.screenshots.folder = Some(screenshots.clone());
        });
        cx.set_global(Casting::local(vec![Arc::new(fake.clone())]));
        Casting::refresh(cx);
        AudioEngine::set_devices(
            vec![DeviceInfo {
                id: "speakers".into(),
                name: "Speakers".into(),
            }],
            cx,
        );
    });
    let settings = cx.update(|cx| cx.global::<Settings>().clone());
    let mut wrong = Vec::new();
    let specs = all().filter(|spec| spec.namespace() == namespace && !spec.query);
    for spec in specs {
        if NOT_RUN.iter().any(|&(id, _)| id == spec.id) {
            assert!(spec.tells.is_empty(), "{} isn't run to hear it", spec.id);
            continue;
        }
        let (heard, mut failed) = hear(spec, &settings, &tv.id, cx);
        wrong.append(&mut failed);
        let unheard = |kind: &&str| {
            !NOT_HEARD
                .iter()
                .any(|&(id, event, _)| id == spec.id && event == *kind)
        };
        let declared: BTreeSet<&str> = spec.tells.iter().copied().filter(unheard).collect();
        if heard != declared {
            wrong.push(format!(
                "{} tells {heard:?}, and says {declared:?}",
                spec.id
            ));
        }
    }
    std::fs::remove_dir_all(&screenshots).ok();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

macro_rules! namespaces {
    ($($test:ident: $namespace:literal),* $(,)?) => {
        $(
            #[gpui::test]
            fn $test(cx: &mut TestAppContext) {
                check($namespace, cx);
            }
        )*

        /// The namespaces each checked by a test of its own.
        const CHECKED: &[&str] = &[$($namespace),*];
    };
}

namespaces! {
    app_commands_tell_what_they_say: "app",
    window_commands_tell_what_they_say: "window",
    view_commands_tell_what_they_say: "view",
    tile_commands_tell_what_they_say: "tile",
    playback_commands_tell_what_they_say: "playback",
    audio_commands_tell_what_they_say: "audio",
    subtitles_commands_tell_what_they_say: "subtitles",
    video_commands_tell_what_they_say: "video",
    cast_commands_tell_what_they_say: "cast",
    output_commands_tell_what_they_say: "output",
    settings_commands_tell_what_they_say: "settings",
    cli_commands_tell_what_they_say: "cli",
}

#[test]
fn every_namespace_that_changes_anything_is_checked() {
    let mut changing: Vec<&str> = all()
        .filter(|spec| !spec.query)
        .map(Spec::namespace)
        .collect();
    changing.dedup();
    assert_eq!(changing, CHECKED);
}

#[test]
fn what_cant_be_heard_here_is_said_and_why() {
    for (id, event, why) in NOT_HEARD {
        let spec = find(id).expect("a command");
        assert!(spec.tells.contains(&event), "{id} says it tells {event}");
        assert!(why.len() > 10);
    }
}
