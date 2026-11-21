//! Smoke runs: the real app, with its own windows, GPU and sound, driven
//! through a scenario that checks what headless tests can't reach, as
//! their windows have no native window behind them. After a window fits
//! its files, gpui must lay it out at the size it really is;
//! playing tiles must show frames; the sound must have a device to go
//! to; and nothing may log an error, whatever `RUST_LOG` lets through.
//!
//! Built with the `smoke` feature, whose settings are the defaults and
//! never saved, and run one scenario a process (`script/smoke` runs them
//! all): `plays --smoke first-open`. It prints what failed and exits with
//! 1, or 0 when all went well. `plays --smoke list` lists them.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gpui::{
    AnyWindowHandle, App, AsyncApp, Entity, Global, Keystroke, Pixels, Size, WindowHandle, px, size,
};

use super::dispatch::first_workspace;
use super::{AudioEngine, Route};
use crate::media::Source;
use crate::media::tracks::TrackKind;
use crate::settings::{FloatOnTop, Settings};
use crate::ui::player::PlayerView;
use crate::ui::settings::SettingsView;
use crate::ui::theme::layout::{WINDOW_MIN_SIZE, WINDOW_SIZE};
use crate::ui::workspace::{TOOLBAR_SPACE, Workspace};

/// How long a whole scenario may take.
const SCENARIO_WAIT: Duration = Duration::from_secs(60);
/// How long to wait for any one thing to happen.
const WAIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(50);
/// How long a window's size must hold still to have finished resizing.
const SETTLED: Duration = Duration::from_millis(500);
/// How long frames are counted for, the fixtures looping once in it.
const WATCH: Duration = Duration::from_secs(2);
/// The fixtures play at 25 frames a second, and a window drawing shows
/// each. One that doesn't, as when it is covered, moves on about 5 a
/// second (`Workspace`'s background ticks).
const MIN_FPS: f64 = 15.0;
/// How far apart two frame times may be and still be the same frame.
const SAME_FRAME: f64 = 0.001;
/// One frame of the fixtures.
const FRAME: f64 = 1.0 / 25.0;

/// Error records logged so far, from any crate.
static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Errors gpui logs about the machine rather than the app, which aren't
/// counted: Xvfb's pointer has no scroll wheel.
const MACHINE_ERRORS: &[(&str, &str)] = if cfg!(target_os = "linux") {
    &[(
        "gpui::platform::linux::x11::client",
        "Found no xinput mouse pointers.",
    )]
} else {
    &[]
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scenario {
    /// A video opened into the empty window, which fits it.
    FirstOpen,
    /// Two videos and a sound in one window.
    Several,
    /// ⌘W closes the tiles, then the window, and the app quits.
    Close,
    /// Subtitles beside the video turn off and on again.
    Subtitles,
    /// Stepping a frame forward and back, through B-frames.
    StepBack,
    /// Floating on top while playing, as the system sees the windows.
    FloatOnTop,
    /// ⌘1 and Ctrl+Tab bringing player windows to the front.
    Windows,
}

impl Scenario {
    pub const ALL: [Self; 7] = [
        Self::FirstOpen,
        Self::Several,
        Self::Close,
        Self::Subtitles,
        Self::StepBack,
        Self::FloatOnTop,
        Self::Windows,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::FirstOpen => "first-open",
            Self::Several => "several",
            Self::Close => "close",
            Self::Subtitles => "subtitles",
            Self::StepBack => "step-back",
            Self::FloatOnTop => "float-on-top",
            Self::Windows => "windows",
        }
    }
}

/// The scenario `--smoke <name>` asks for, if it is the first argument,
/// and the arguments after it. Exits, listing them, for `list` or one not
/// known.
pub fn from_args(args: Vec<OsString>) -> (Option<Scenario>, Vec<OsString>) {
    if args.first().is_none_or(|arg| arg != "--smoke") {
        return (None, args);
    }
    let name = args.get(1).and_then(|name| name.to_str()).unwrap_or("");
    if name == "list" {
        for scenario in Scenario::ALL {
            println!("{}", scenario.name());
        }
        std::process::exit(0);
    }
    let Some(scenario) = Scenario::ALL.into_iter().find(|s| s.name() == name) else {
        let names: Vec<_> = Scenario::ALL.iter().map(|s| s.name()).collect();
        eprintln!("usage: plays --smoke <{}>", names.join("|"));
        std::process::exit(2);
    };
    (Some(scenario), args.into_iter().skip(2).collect())
}

/// Log as `RUST_LOG` says, defaulting to Plays' info, and keep every
/// error, from any crate, whatever it says.
pub fn init_logging() {
    let inner =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("plays=info"))
            .build();
    let level = inner.filter().max(log::LevelFilter::Error);
    log::set_boxed_logger(Box::new(Logger(inner))).expect("setting the logger");
    log::set_max_level(level);
}

struct Logger(env_logger::Logger);

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() == log::Level::Error || self.0.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if record.level() == log::Level::Error {
            let message = record.args().to_string();
            let machine = MACHINE_ERRORS
                .iter()
                .any(|&(target, error)| record.target() == target && message == error);
            if !machine {
                let error = format!("{}: {message}", record.target());
                if !self.0.enabled(record.metadata()) {
                    eprintln!("ERROR {error}");
                }
                ERRORS.lock().unwrap().push(error);
            }
        }
        self.0.log(record);
    }

    fn flush(&self) {
        self.0.flush();
    }
}

/// Set while the app is expected to quit, its last window closed.
struct QuitExpected;
impl Global for QuitExpected {}

/// Run `scenario` in the app `app::run` started, then exit.
pub fn start(scenario: Scenario, cx: &mut App) {
    // The tiny fixtures run out in 2 seconds: keep them playing. The sound
    // goes to the device all the same, but at no volume.
    Settings::update(cx, false, |settings| {
        settings.loop_by_default = true;
        settings.master_volume = 0.0;
    });
    let started = Instant::now();
    cx.on_app_quit(move |cx| {
        let result = if cx.has_global::<QuitExpected>() {
            Ok(())
        } else {
            Err("the app quit".to_string())
        };
        finish(scenario, started, result);
        #[allow(unreachable_code)]
        async {}
    })
    .detach();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SCENARIO_WAIT).await;
        finish(
            scenario,
            started,
            Err(format!("didn't finish in {SCENARIO_WAIT:?}")),
        )
    })
    .detach();
    cx.spawn(async move |cx| {
        let result = match scenario {
            Scenario::FirstOpen => first_open(cx).await,
            Scenario::Several => several(cx).await,
            Scenario::Close => close(cx).await,
            Scenario::Subtitles => subtitles(cx).await,
            Scenario::StepBack => step_back(cx).await,
            Scenario::FloatOnTop => float_on_top(cx).await,
            Scenario::Windows => windows(cx).await,
        };
        finish(scenario, started, result)
    })
    .detach();
}

/// Report how `scenario` went, with any errors logged, and exit.
fn finish(scenario: Scenario, started: Instant, result: Result<(), String>) -> ! {
    log::logger().flush();
    let mut failures: Vec<String> = result.err().into_iter().collect();
    failures.extend(
        ERRORS
            .lock()
            .unwrap()
            .iter()
            .map(|error| format!("logged an error: {error}")),
    );
    let name = scenario.name();
    let took = started.elapsed().as_secs_f64();
    if failures.is_empty() {
        println!("smoke {name}: ok in {took:.1}s");
        std::process::exit(0);
    }
    eprintln!("smoke {name}: FAILED after {took:.1}s");
    for failure in failures {
        eprintln!("  {failure}");
    }
    std::process::exit(1);
}

async fn first_open(cx: &mut AsyncApp) -> Result<(), String> {
    open(cx, &["clip.mp4"]).await?;
    let fitted = settle(cx).await?;
    // At its smallest, as the 160×120 video is smaller still.
    let expected = size(
        px(160.).max(WINDOW_MIN_SIZE.width),
        (px(120.) + TOOLBAR_SPACE).max(WINDOW_MIN_SIZE.height),
    );
    if fitted != expected {
        return Err(format!(
            "the window is {fitted:?} after fitting, not {expected:?}"
        ));
    }
    check_playing(cx).await?;
    check_route(cx)
}

async fn several(cx: &mut AsyncApp) -> Result<(), String> {
    open(cx, &["clip.mp4", "clip-ip.mkv", "tone.m4a"]).await?;
    let fitted = settle(cx).await?;
    if fitted == WINDOW_SIZE {
        return Err(format!("the window kept its size, {fitted:?}"));
    }
    check_playing(cx).await?;
    check_route(cx)
}

async fn close(cx: &mut AsyncApp) -> Result<(), String> {
    open(cx, &["clip.mp4", "clip-ip.mkv"]).await?;
    settle(cx).await?;
    let close = "secondary-w";

    press(cx, close)?;
    wait_for(cx, "⌘W to close a tile", |cx| players(cx).len() == 1).await?;
    settle(cx).await?;
    check_playing(cx).await?;

    press(cx, close)?;
    wait_for(cx, "⌘W to close the last tile", |cx| {
        players(cx).is_empty()
    })
    .await?;
    settle(cx).await?;

    // The empty window closes, and with it the app.
    cx.update(|cx| cx.set_global(QuitExpected))
        .map_err(|_| "the app went away")?;
    press(cx, close)?;
    sleep(cx, WAIT).await;
    Err("the app didn't quit after its last window closed".into())
}

async fn subtitles(cx: &mut AsyncApp) -> Result<(), String> {
    // clip.srt, beside it, has "First" from 0.2 to 1.0s.
    open(cx, &["clip.mp4"]).await?;
    settle(cx).await?;
    check_playing(cx).await?;
    let listed = with_player(cx, |view, _| {
        view.player()
            .info()
            .is_some_and(|info| info.tracks(TrackKind::Subtitle).count() == 1)
    })?;
    if !listed {
        return Err("clip.srt isn't listed beside clip.mp4".into());
    }
    press_for(cx, "playback.play_pause")?;
    wait_for(cx, "the video to pause", |cx| {
        player_state(cx, |view| !view.player().is_playing())
    })
    .await?;
    with_player(cx, |view, cx| {
        view.seek(0.5);
        cx.notify();
    })?;
    wait_for(cx, "\"First\" to show at 0.5s", |cx| {
        showing(cx) == ["First"]
    })
    .await?;

    press_for(cx, "subtitles.next_track")?;
    wait_for(cx, "the subtitles to turn off", |cx| {
        selected_subtitles(cx).is_none() && showing(cx).is_empty()
    })
    .await?;
    press_for(cx, "subtitles.next_track")?;
    wait_for(cx, "the subtitles to come back", |cx| {
        selected_subtitles(cx).is_some() && showing(cx) == ["First"]
    })
    .await?;
    check_route(cx)
}

async fn step_back(cx: &mut AsyncApp) -> Result<(), String> {
    // clip.mp4 has B-frames, so a frame back is decoded from the keyframe
    // before it.
    open(cx, &["clip.mp4"]).await?;
    settle(cx).await?;
    check_playing(cx).await?;
    press_for(cx, "playback.play_pause")?;
    wait_for(cx, "the video to pause", |cx| {
        player_state(cx, |view| !view.player().is_playing())
    })
    .await?;
    with_player(cx, |view, cx| {
        view.seek(1.0);
        cx.notify();
    })?;
    let mut at = wait_for_frame(cx, 1.0).await?;
    for (command, step) in [
        ("playback.next_frame", 1.0),
        ("playback.previous_frame", -1.0),
        ("playback.previous_frame", -1.0),
    ] {
        press_for(cx, command)?;
        at = wait_for_frame(cx, at + step * FRAME).await?;
    }
    check_route(cx)
}

async fn float_on_top(cx: &mut AsyncApp) -> Result<(), String> {
    open(cx, &["clip.mp4"]).await?;
    settle(cx).await?;
    check_playing(cx).await?;
    if on_top(cx, false)? {
        return Err("the window floats before it is asked to".into());
    }
    set_float_on_top(cx, FloatOnTop::WhilePlaying)?;
    wait_for(cx, "the window to float while it plays", |cx| {
        is_on_top(cx, false)
    })
    .await?;
    if !in_mission_control(cx, false)? {
        return Err("the window floating is missing from Mission Control".into());
    }
    press_for(cx, "playback.play_pause")?;
    wait_for(cx, "the window to come down when paused", |cx| {
        !is_on_top(cx, false)
    })
    .await?;
    press_for(cx, "playback.play_pause")?;
    wait_for(cx, "the window to float again when playing", |cx| {
        is_on_top(cx, false)
    })
    .await?;

    // Settings open over the window, floating with it.
    cx.update(SettingsView::open_window)
        .map_err(|_| "the app went away")?;
    wait_for(cx, "Settings to float with the window", |cx| {
        is_on_top(cx, true)
    })
    .await?;
    if !in_mission_control(cx, true)? {
        return Err("Settings floating is missing from Mission Control".into());
    }
    set_float_on_top(cx, FloatOnTop::Off)?;
    wait_for(cx, "both windows to come down", |cx| {
        !is_on_top(cx, false) && !is_on_top(cx, true)
    })
    .await?;
    check_route(cx)
}

async fn windows(cx: &mut AsyncApp) -> Result<(), String> {
    open(cx, &["clip.mp4"]).await?;
    settle(cx).await?;
    let first = cx
        .update(|cx| workspace(cx))
        .map_err(|_| "the app went away")??
        .window_id();
    let second = cx
        .update(|cx| Workspace::open_window(WINDOW_SIZE, cx))
        .map_err(|_| "the app went away")?
        .map_err(|err| format!("can't open a second window: {err:#}"))?
        .window_id();
    let front_is = |id| move |cx: &mut App| cx.active_window().map(|w| w.window_id()) == Some(id);
    wait_for(cx, "the second window to open in front", front_is(second)).await?;

    press_in_front(cx, "secondary-1")?;
    wait_for(
        cx,
        "⌘1 to bring the first window to the front",
        front_is(first),
    )
    .await?;
    press_in_front(cx, "ctrl-tab")?;
    wait_for(
        cx,
        "Ctrl+Tab to bring the second to the front",
        front_is(second),
    )
    .await?;
    press_in_front(cx, "ctrl-shift-tab")?;
    wait_for(
        cx,
        "Ctrl+Shift+Tab to bring the first back",
        front_is(first),
    )
    .await?;
    check_playing(cx).await?;
    check_route(cx)
}

fn set_float_on_top(cx: &mut AsyncApp, mode: FloatOnTop) -> Result<(), String> {
    cx.update(|cx| Settings::update(cx, false, |s| s.float_on_top = mode))
        .map_err(|_| "the app went away".into())
}

/// Whether the player window, or with `settings` the Settings window,
/// floats above other apps' windows, as the system has it.
fn on_top(cx: &mut AsyncApp, settings: bool) -> Result<bool, String> {
    cx.update(|cx| is_on_top(cx, settings))
        .map_err(|_| "the app went away".into())
}

/// Whether Mission Control shows the player window, or with `settings` the
/// Settings window, and ⌘` goes to it, as the system has it.
fn in_mission_control(cx: &mut AsyncApp, settings: bool) -> Result<bool, String> {
    cx.update(|cx| {
        player_or_settings(cx, settings).is_some_and(|window| {
            window
                .update(cx, |_, window, _| window.is_in_mission_control())
                .unwrap_or(false)
        })
    })
    .map_err(|_| "the app went away".into())
}

fn is_on_top(cx: &mut App, settings: bool) -> bool {
    player_or_settings(cx, settings).is_some_and(|window| {
        window
            .update(cx, |_, window, _| window.is_on_top())
            .unwrap_or(false)
    })
}

/// The player window, or with `settings` the Settings window.
fn player_or_settings(cx: &mut App, settings: bool) -> Option<AnyWindowHandle> {
    if settings {
        cx.windows()
            .into_iter()
            .find(|window| window.downcast::<SettingsView>().is_some())
    } else {
        workspace(cx).ok().map(AnyWindowHandle::from)
    }
}

/// Open the fixtures `files` into the window, as Finder does, and wait
/// until they have.
async fn open(cx: &mut AsyncApp, files: &[&str]) -> Result<(), String> {
    let sources = files
        .iter()
        .map(|file| {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(file);
            Source::from_arg(path)
        })
        .collect();
    cx.update(|cx| {
        workspace(cx)?
            .update(cx, |workspace, _, cx| workspace.open_sources(sources, cx))
            .map_err(|_| "the window went away".to_string())
    })
    .map_err(|_| "the app went away")??;
    wait_for(cx, "the files to open", |cx| {
        let players = players(cx);
        players.len() == files.len()
            && players.iter().all(|view| {
                let player = view.read(cx).player();
                player.is_known() || player.error().is_some()
            })
    })
    .await?;
    let errors = cx
        .update(|cx| {
            players(cx)
                .iter()
                .filter_map(|view| view.read(cx).player().error())
                .collect::<Vec<_>>()
        })
        .map_err(|_| "the app went away")?;
    match errors.first() {
        Some(error) => Err(format!("a file didn't open: {error}")),
        None => Ok(()),
    }
}

/// Wait for the window to fit what it shows and its size to hold still,
/// then check that gpui lays it out at that size; its size.
async fn settle(cx: &mut AsyncApp) -> Result<Size<Pixels>, String> {
    wait_for(cx, "the window to fit its files", |cx| {
        workspace(cx).is_ok_and(|window| window.read(cx).is_ok_and(|w| !w.is_fitting()))
    })
    .await?;
    let deadline = Instant::now() + WAIT;
    let mut last = sizes(cx)?;
    let mut still_since = Instant::now();
    while still_since.elapsed() < SETTLED {
        if Instant::now() > deadline {
            return Err(format!("the window kept resizing for {WAIT:?}"));
        }
        sleep(cx, POLL).await;
        let now = sizes(cx)?;
        if now.0 != last.0 {
            still_since = Instant::now();
        }
        last = now;
    }
    let (native, laid_out) = last;
    let off = (native.width - laid_out.width)
        .abs()
        .max((native.height - laid_out.height).abs());
    if off > px(0.5) {
        return Err(format!(
            "the window is {native:?}, but gpui lays it out at {laid_out:?}"
        ));
    }
    Ok(native)
}

/// The window's size as the system has it, and as gpui lays it out.
fn sizes(cx: &mut AsyncApp) -> Result<(Size<Pixels>, Size<Pixels>), String> {
    cx.update(|cx| {
        workspace(cx)?
            .update(cx, |_, window, _| {
                (window.bounds().size, window.viewport_size())
            })
            .map_err(|_| "the window went away".to_string())
    })
    .map_err(|_| "the app went away")?
}

/// Check that every tile with video plays, and that the window draws
/// each of its frames.
async fn check_playing(cx: &mut AsyncApp) -> Result<(), String> {
    let start = video_tiles(cx)?;
    sleep(cx, WATCH).await;
    let end = video_tiles(cx)?;
    for (before, after) in start.into_iter().zip(end) {
        let name = after.name;
        if !after.playing {
            return Err(format!("{name} isn't playing"));
        }
        let fps = (after.shown - before.shown) as f64 / WATCH.as_secs_f64();
        if fps < MIN_FPS {
            return Err(format!(
                "{name} showed {fps:.1} frames a second, under {MIN_FPS}: \
                 the window doesn't draw (is it covered?)"
            ));
        }
    }
    Ok(())
}

/// How a tile with video plays.
struct VideoTile {
    name: String,
    playing: bool,
    /// Frames shown since it opened.
    shown: u64,
}

fn video_tiles(cx: &mut AsyncApp) -> Result<Vec<VideoTile>, String> {
    cx.update(|cx| {
        players(cx)
            .iter()
            .map(|view| view.read(cx).player())
            .filter(|player| player.has_video())
            .map(|player| VideoTile {
                name: player.source().name(),
                playing: player.is_playing(),
                shown: player.stats().shown,
            })
            .collect()
    })
    .map_err(|_| "the app went away".to_string())
}

/// Check that the sound goes to a device.
fn check_route(cx: &mut AsyncApp) -> Result<(), String> {
    let route = cx
        .update(|cx| AudioEngine::route(cx).cloned())
        .map_err(|_| "the app went away")?;
    match route {
        Some(Route::Device { .. }) => Ok(()),
        other => Err(format!("the sound has nowhere to go: {other:?}")),
    }
}

/// Wait for the frame showing `time`, and return its time.
async fn wait_for_frame(cx: &mut AsyncApp, time: f64) -> Result<f64, String> {
    let what = format!("the frame at {time:.3}s");
    let result = wait_for(cx, &what, |cx| {
        frame_time(cx).is_some_and(|at| (at - time).abs() < SAME_FRAME)
    })
    .await;
    let at = cx.update(frame_time).ok().flatten();
    result
        .map_err(|err| format!("{err}, showing {at:?}"))
        .map(|_| at.unwrap_or(time))
}

fn frame_time(cx: &mut App) -> Option<f64> {
    players(cx).first()?.read(cx).player().frame_pts()
}

fn selected_subtitles(cx: &mut App) -> Option<usize> {
    let info = players(cx).first()?.read(cx).player().info()?;
    info.selected(TrackKind::Subtitle)
}

/// The text of the subtitles showing in the first tile.
fn showing(cx: &mut App) -> Vec<String> {
    players(cx).first().map_or(Vec::new(), |view| {
        view.read(cx)
            .player()
            .subtitles()
            .iter()
            .flat_map(|cue| cue.texts.iter().map(|text| text.text.clone()))
            .collect()
    })
}

fn player_state(cx: &mut App, state: impl Fn(&PlayerView) -> bool) -> bool {
    players(cx).first().is_some_and(|view| state(view.read(cx)))
}

/// Change the first tile's player.
fn with_player<R>(
    cx: &mut AsyncApp,
    change: impl FnOnce(&mut PlayerView, &mut gpui::Context<PlayerView>) -> R,
) -> Result<R, String> {
    cx.update(|cx| {
        let view = players(cx).into_iter().next().ok_or("no tile")?;
        Ok(view.update(cx, change))
    })
    .map_err(|_| "the app went away".to_string())?
}

fn workspace(cx: &App) -> Result<WindowHandle<Workspace>, String> {
    first_workspace(cx.windows()).ok_or_else(|| "no window".to_string())
}

/// The window's tiles' players, in order.
fn players(cx: &App) -> Vec<Entity<PlayerView>> {
    workspace(cx)
        .ok()
        .and_then(|window| window.read(cx).ok())
        .map_or(Vec::new(), |workspace| workspace.players())
}

/// Press `key` in the window, as typed on the keyboard. Through the
/// window, not its root view, which the key's action may update.
fn press(cx: &mut AsyncApp, key: &str) -> Result<(), String> {
    let window = cx
        .update(|cx| workspace(cx))
        .map_err(|_| "the app went away")??;
    press_in(cx, window.into(), key)
}

/// Press `key` in the window in front, which may not be the player's.
fn press_in_front(cx: &mut AsyncApp, key: &str) -> Result<(), String> {
    let window = cx
        .update(|cx| cx.active_window())
        .map_err(|_| "the app went away")?
        .ok_or("no window is in front")?;
    press_in(cx, window, key)
}

fn press_in(cx: &mut AsyncApp, window: AnyWindowHandle, key: &str) -> Result<(), String> {
    let keystroke = Keystroke::parse(key).map_err(|err| format!("the key {key}: {err}"))?;
    window
        .update(cx, |_, window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        })
        .map_err(|_| "the window went away".to_string())
}

/// Press the first key Settings give `command`, by its id.
fn press_for(cx: &mut AsyncApp, command: &str) -> Result<(), String> {
    let key = cx
        .update(|cx| {
            cx.global::<Settings>()
                .hotkeys
                .keys(command)
                .first()
                .cloned()
        })
        .map_err(|_| "the app went away")?
        .ok_or_else(|| format!("{command} has no key"))?;
    press(cx, &key)
}

/// Wait until `done`, for up to `WAIT`.
async fn wait_for(
    cx: &mut AsyncApp,
    what: &str,
    mut done: impl FnMut(&mut App) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + WAIT;
    loop {
        if cx.update(&mut done).map_err(|_| "the app went away")? {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("waited {WAIT:?} for {what}"));
        }
        sleep(cx, POLL).await;
    }
}

async fn sleep(cx: &AsyncApp, duration: Duration) {
    cx.background_executor().timer(duration).await;
}
