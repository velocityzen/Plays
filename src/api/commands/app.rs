//! `app`: the app's own, and opening files and streams.

use std::path::Path;

use gpui::{App, ClipboardItem};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Switch, output, panels, settings, window};
use crate::api::Error;
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Args, Outcome, Scope, Spec};
use crate::api::target::{self, Target};
use crate::app::{AudioEngine, updates};
use crate::media::Source;
use crate::recents;
use crate::settings::{Panel, Settings, SeveralFiles};
use crate::ui::about::{self, AboutView};
use crate::ui::sentence::capitalized;
use crate::ui::settings::{ABOUT, DEFAULT_PLAYER, SettingsView, ffmpeg_version};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::{Opening, Workspace};

pub const COMMANDS: &[Spec] = &[
    Spec::query(
        "app.inspect",
        "Inspect Plays",
        "Say what Plays is: its version, its player windows, and where the sound goes.",
        Scope::App,
        "{version, windows: [as window.ls], output: as output.ls}.",
        |_, _, cx| {
            Ok(json!({
                "version": env!("CARGO_PKG_VERSION"),
                "windows": window::summaries(cx)?,
                "output": output::listing(cx),
            }))
        },
    )
    .keywords(&["info", "version", "about", "status"]),
    Spec::query(
        "app.formats",
        "List Formats",
        "List the formats Plays opens, video and audio.",
        Scope::App,
        "[{name, kind, extensions}]: what each is called, video or audio, and the \
         extensions of its files, lowercase.",
        |_, _, _| Ok(formats()),
    )
    .keywords(&["formats", "extensions", "file types", "video", "audio"]),
    Spec::custom_with(
        "app.open",
        "Open Paths or URLs",
        "Play files, the media files in folders, and streams, in the window named, or \
         the one in front, or a new one if none is open: each in a block, or one after \
         another in one, as Settings say.",
        Scope::Window,
        &[
            Arg::required(
                "urls",
                Kind::Texts,
                "Each a file's or folder's full path, a file:// URL, or a stream's URL.",
            ),
            Arg::optional(
                "autoplay",
                Kind::Toggle,
                "Whether each plays as it opens, or opens paused: as Settings say if not given.",
            ),
            Arg::optional(
                "several_files",
                Kind::Choice(|| vec!["together", "playlist"]),
                "Several, or a folder's, each in a block of its own (together), or in one \
                 playing them one after another (playlist): as Settings say if not given.",
            ),
            Arg::optional(
                "kind",
                Kind::Choice(|| vec!["video", "audio"]),
                "Only the video files in folders, or only the sound files: all their media \
                 files if not given.",
            ),
        ],
        open,
    )
    .tells(&[
        "window.created",
        "window.active_changed",
        "block.created",
        "window.layout_changed",
        "block.active_changed",
        "playback.opened",
    ])
    .keywords(&["file", "folder", "stream", "url", "play", "add"])
    .returns("{window}: the id of the window they open in."),
    Spec::app(
        "app.clear_recent_files",
        "Clear Recent Files",
        "Forget the files played lately, which File → Open Recent and Go to… offer.",
        |cx| recents::FILES.clear(cx),
    )
    .keywords(&["recent", "history", "forget", "open recent"]),
    Spec::app(
        "app.about",
        "About Plays",
        "Show Plays' version, and what it's made with.",
        AboutView::open_window,
    )
    .keywords(&["version", "credits", "licenses"]),
    Spec::app(
        "app.default_player",
        "Make Plays the Default Player…",
        "Open Settings at Default Player, to pick the formats Plays opens when their files \
         are opened: from Finder or a file manager, say.",
        |cx| SettingsView::open_at(DEFAULT_PLAYER, cx),
    )
    .keywords(&[
        "default",
        "open with",
        "file types",
        "formats",
        "associations",
    ]),
    Spec::app(
        "app.check_for_updates",
        "Check for Updates…",
        "Open Settings at About, and check for a newer Plays: one found downloads in the \
         background, to install as Plays restarts or quits.",
        |cx| {
            SettingsView::open_at(ABOUT, cx);
            updates::check(cx);
        },
    )
    .keywords(&["update", "upgrade", "new version", "download"]),
    Spec::app_with(
        "app.restart_to_update",
        "Restart to Update",
        "Install the update downloaded, and start Plays again to run it.",
        &[],
        |_, cx| {
            updates::restart(cx).map_err(|why| Error::failed(capitalized(&why)))?;
            Ok(Value::Null)
        },
    )
    .keywords(&["update", "upgrade", "install", "relaunch"]),
    Spec::app_with(
        "app.copy_version_info",
        "Copy Version Info",
        "Copy Plays' version and the day it was built.",
        &[],
        |_, cx| Ok(json!(copy(version_info(), cx))),
    )
    .keywords(&["version", "about", "clipboard"])
    .returns("What it copied, such as Plays 0.1.0 (18 November 2026)."),
    Spec::app_with(
        "app.copy_debug_info",
        "Copy Debug Info",
        "Copy what a bug report needs: Plays' version, the system, FFmpeg's version, \
         where the sound goes, the windows and blocks, and the settings changed.",
        &[],
        |_, cx| {
            let info = debug_info(cx)?;
            Ok(json!(copy(info, cx)))
        },
    )
    .keywords(&["bug", "report", "diagnostics", "clipboard", "system"])
    .returns("What it copied, a line for each thing."),
    // Debug builds only, from the Developer menu.
    #[cfg(debug_assertions)]
    Spec::app(
        "app.show_components",
        "Component Gallery",
        "Show every component in each of its sizes and states, side by side.",
        crate::ui::gallery::GalleryView::open_window,
    )
    .keywords(&["developer", "debug", "components", "gallery", "sizes"])
    .developer(),
    Spec::app(
        "app.ui.toggle",
        "Show/Hide All in Every Window",
        "Hide every panel over the video in every window while Settings show any, else \
         show them all.",
        |cx| switch_panel(Switch::Toggle, None, cx),
    )
    .tells(TELLS_ALL)
    .keywords(&["hide", "interface", "ui", "panels", "clean"])
    .setting("app.toggle_ui"),
    Spec::app(
        "app.ui.on",
        "Show All in Every Window",
        "Show every panel over the video in every window, and those opened later.",
        |cx| switch_panel(Switch::On, None, cx),
    )
    .tells(TELLS_ALL_ON)
    .keywords(&["interface", "ui", "panels"]),
    Spec::app(
        "app.ui.off",
        "Hide All in Every Window",
        "Hide every panel over the video in every window, and those opened later.",
        |cx| switch_panel(Switch::Off, None, cx),
    )
    .tells(TELLS_ALL_OFF)
    .keywords(&["interface", "ui", "panels", "clean"]),
    Spec::app(
        "app.controls.toggle",
        "Show/Hide Controls in Every Window",
        "Show the seek bar and buttons at the bottom of every block in every window, \
         and those opened later, or hide them, as Settings → Appearance does.",
        |cx| switch_panel(Switch::Toggle, Some(panels::controls), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.controls.on",
        "app.controls.off",
        "window.controls.on",
        "window.controls.off",
        "block.controls.on",
        "block.controls.off",
    ])
    .keywords(&["hide", "player controls", "seek bar", "bottom bar"])
    .setting("app.toggle_controls")
    .state(|_, cx| Ok(json!(cx.global::<Settings>().over_video.controls))),
    Spec::app(
        "app.controls.on",
        "Show Controls in Every Window",
        "Show the seek bar and buttons at the bottom of every block in every window, \
         and those opened later, as Settings → Appearance does.",
        |cx| switch_panel(Switch::On, Some(panels::controls), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.controls.on",
        "window.controls.on",
        "block.controls.on",
    ])
    .keywords(&["player controls", "seek bar", "bottom bar"]),
    Spec::app(
        "app.controls.off",
        "Hide Controls in Every Window",
        "Hide the seek bar and buttons at the bottom of every block in every window, \
         and those opened later, as Settings → Appearance does.",
        |cx| switch_panel(Switch::Off, Some(panels::controls), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.controls.off",
        "window.controls.off",
        "block.controls.off",
    ])
    .keywords(&["player controls", "seek bar", "bottom bar"]),
    Spec::app(
        "app.top_bar.toggle",
        "Show/Hide Top Bar in Every Window",
        "Show the file's name and buttons at the top of every block in every window, \
         and those opened later, or hide them, as Settings → Appearance does.",
        |cx| switch_panel(Switch::Toggle, Some(panels::top_bar), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.top_bar.on",
        "app.top_bar.off",
        "window.top_bar.on",
        "window.top_bar.off",
        "block.top_bar.on",
        "block.top_bar.off",
    ])
    .keywords(&["hide", "header", "title", "buttons"])
    .setting("app.toggle_top_bar")
    .state(|_, cx| Ok(json!(cx.global::<Settings>().over_video.top_bar))),
    Spec::app(
        "app.top_bar.on",
        "Show Top Bar in Every Window",
        "Show the file's name and buttons at the top of every block in every window, \
         and those opened later, as Settings → Appearance does.",
        |cx| switch_panel(Switch::On, Some(panels::top_bar), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.top_bar.on",
        "window.top_bar.on",
        "block.top_bar.on",
    ])
    .keywords(&["header", "title", "buttons"]),
    Spec::app(
        "app.top_bar.off",
        "Hide Top Bar in Every Window",
        "Hide the file's name and buttons at the top of every block in every window, \
         and those opened later, as Settings → Appearance does.",
        |cx| switch_panel(Switch::Off, Some(panels::top_bar), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.top_bar.off",
        "window.top_bar.off",
        "block.top_bar.off",
    ])
    .keywords(&["header", "title", "buttons"]),
    Spec::app(
        "app.track_switcher.toggle",
        "Show/Hide Track Switcher in Every Window",
        "Show the reel of tracks a shortcut rolls to the audio, video or subtitles \
         picked, over every block in every window, and those opened later, or switch \
         without it, as Settings → Appearance does.",
        |cx| switch_panel(Switch::Toggle, Some(panels::track_switcher), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.track_switcher.on",
        "app.track_switcher.off",
        "window.track_switcher.on",
        "window.track_switcher.off",
        "block.track_switcher.on",
        "block.track_switcher.off",
    ])
    .keywords(&["hide", "reel", "streams", "audio track", "subtitle track"])
    .setting("app.toggle_track_switcher")
    .state(|_, cx| Ok(json!(cx.global::<Settings>().over_video.track_switcher))),
    Spec::app(
        "app.track_switcher.on",
        "Show Track Switcher in Every Window",
        "Show the reel of tracks a shortcut rolls to the audio, video or subtitles \
         picked, over every block in every window, and those opened later, as \
         Settings → Appearance does.",
        |cx| switch_panel(Switch::On, Some(panels::track_switcher), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.track_switcher.on",
        "window.track_switcher.on",
        "block.track_switcher.on",
    ])
    .keywords(&["reel", "streams", "audio track", "subtitle track"]),
    Spec::app(
        "app.track_switcher.off",
        "Hide Track Switcher in Every Window",
        "Switch tracks without the reel in every block in every window, and those \
         opened later, as Settings → Appearance does.",
        |cx| switch_panel(Switch::Off, Some(panels::track_switcher), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.track_switcher.off",
        "window.track_switcher.off",
        "block.track_switcher.off",
    ])
    .keywords(&["reel", "streams", "audio track", "subtitle track"]),
    Spec::app(
        "app.notes.toggle",
        "Show/Hide Notes in Every Window",
        "Show notes on what a shortcut changed, like the volume, over every block in \
         every window, and those opened later, or not, as Settings → Appearance does. \
         Notes on what's wrong still show.",
        |cx| switch_panel(Switch::Toggle, Some(panels::notes), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.notes.on",
        "app.notes.off",
        "window.notes.on",
        "window.notes.off",
        "block.notes.on",
        "block.notes.off",
    ])
    .keywords(&["hide", "osd", "on-screen display", "volume", "messages"])
    .setting("app.toggle_notes")
    .state(|_, cx| Ok(json!(cx.global::<Settings>().over_video.notes))),
    Spec::app(
        "app.notes.on",
        "Show Notes in Every Window",
        "Show notes on what a shortcut changed, like the volume, over every block in \
         every window, and those opened later, as Settings → Appearance does.",
        |cx| switch_panel(Switch::On, Some(panels::notes), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.notes.on",
        "window.notes.on",
        "block.notes.on",
    ])
    .keywords(&["osd", "on-screen display", "volume", "messages"]),
    Spec::app(
        "app.notes.off",
        "Hide Notes in Every Window",
        "Hide notes on what a shortcut changed over every block in every window, and \
         those opened later, as Settings → Appearance does. Notes on what's wrong \
         still show.",
        |cx| switch_panel(Switch::Off, Some(panels::notes), cx),
    )
    .tells(&[
        "app.settings.changed",
        "app.notes.off",
        "window.notes.off",
        "block.notes.off",
    ])
    .keywords(&["osd", "on-screen display", "volume", "messages"]),
    Spec::app(
        "app.quit",
        "Quit Plays",
        "Quit, keeping where each file got to and stopping the devices cast to.",
        |cx| cx.quit(),
    )
    .tells(&[
        "block.closed",
        "window.closed",
        "window.active_changed",
        "app.quit",
    ])
    .keywords(&["exit"])
    .keys(&["secondary-q"]),
];

/// What every panel over the video in every window showing or hiding
/// tells.
const TELLS_ALL: &[&str] = &[
    "app.settings.changed",
    "app.ui.on",
    "app.ui.off",
    "app.controls.on",
    "app.controls.off",
    "app.top_bar.on",
    "app.top_bar.off",
    "app.track_switcher.on",
    "app.track_switcher.off",
    "app.notes.on",
    "app.notes.off",
    "window.ui.on",
    "window.ui.off",
    "window.controls.on",
    "window.controls.off",
    "window.top_bar.on",
    "window.top_bar.off",
    "window.track_switcher.on",
    "window.track_switcher.off",
    "window.notes.on",
    "window.notes.off",
    "block.ui.on",
    "block.ui.off",
    "block.controls.on",
    "block.controls.off",
    "block.top_bar.on",
    "block.top_bar.off",
    "block.track_switcher.on",
    "block.track_switcher.off",
    "block.notes.on",
    "block.notes.off",
];

/// What every panel over the video in every window showing tells.
const TELLS_ALL_ON: &[&str] = &[
    "app.settings.changed",
    "app.ui.on",
    "app.controls.on",
    "app.top_bar.on",
    "app.track_switcher.on",
    "app.notes.on",
    "window.ui.on",
    "window.controls.on",
    "window.top_bar.on",
    "window.track_switcher.on",
    "window.notes.on",
    "block.ui.on",
    "block.controls.on",
    "block.top_bar.on",
    "block.track_switcher.on",
    "block.notes.on",
];

/// What every panel over the video in every window hiding tells.
const TELLS_ALL_OFF: &[&str] = &[
    "app.settings.changed",
    "app.ui.off",
    "app.controls.off",
    "app.top_bar.off",
    "app.track_switcher.off",
    "app.notes.off",
    "window.ui.off",
    "window.controls.off",
    "window.top_bar.off",
    "window.track_switcher.off",
    "window.notes.off",
    "block.ui.off",
    "block.controls.off",
    "block.top_bar.off",
    "block.track_switcher.off",
    "block.notes.off",
];

/// The app's events.
pub const EVENTS: &[EventKind] = &[
    EventKind::app(
        "app.command_run",
        "A command is about to run: told before anything it changes.",
        &[
            EventField::new("command", "Its id."),
            EventField::new(
                "source",
                "Where it was run from: action (a key, a menu item, or a control that sends one), \
                 ui (a control that runs it itself), palette, or cli.",
            ),
            EventField::optional("args", "The arguments it was given, if any."),
            EventField::optional(
                "target",
                "The window and the block it was aimed at, by name, if either was.",
            ),
        ],
    )
    .causes(&["every command, as it runs"]),
    EventKind::app(
        "app.client_connected",
        "A client connected to the socket, as the command line does.",
        &[EventField::new(
            "client_id",
            "A number for it, which its `app.client_left` has too.",
        )],
    )
    .causes(&["a client connecting"]),
    EventKind::app(
        "app.client_left",
        "A client went from the socket.",
        &[EventField::new(
            "client_id",
            "The number its `app.client_connected` had.",
        )],
    )
    .causes(&[
        "a client going or being cut off, as when control from the command line is turned off",
    ]),
    EventKind::app(
        "app.ui.on",
        "A panel showed over the video in every window, as Settings have them, where none did.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::app(
        "app.ui.off",
        "Every panel over the video in every window, as Settings have them, hid.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::app(
        "app.controls.on",
        "The seek bar and buttons at the bottom of every block, as Settings have them, showed.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::app(
        "app.controls.off",
        "The seek bar and buttons at the bottom of every block, as Settings have them, hid.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::app(
        "app.top_bar.on",
        "The file's name and buttons at the top of every block, as Settings have them, showed.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::app(
        "app.top_bar.off",
        "The file's name and buttons at the top of every block, as Settings have them, hid.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::app(
        "app.track_switcher.on",
        "Every block, as Settings have it, shows the reel of tracks as a shortcut rolls to one.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::app(
        "app.track_switcher.off",
        "Every block, as Settings have it, switches tracks without the reel.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::app(
        "app.notes.on",
        "Every block, as Settings have it, shows notes on what a shortcut changed.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::app(
        "app.notes.off",
        "Every block, as Settings have it, shows no notes on what a shortcut changed.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::to_everyone(
        "app.quit",
        "Plays is quitting: told last, after each window still open is told closing.",
        &[],
    )
    .causes(&["Plays quitting from the Dock or with the system"]),
];

/// Put `text` on the clipboard, handing it back.
fn copy(text: String, cx: &mut App) -> String {
    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
    text
}

/// "Plays 0.1.0 (18 November 2026)".
fn version_info() -> String {
    format!(
        "Plays {} ({})",
        env!("CARGO_PKG_VERSION"),
        about::release_date()
    )
}

/// The version, the system, FFmpeg's version, where the sound goes, how
/// many windows and blocks there are, and each setting changed.
fn debug_info(cx: &App) -> Result<String, Error> {
    let windows = window::summaries(cx)?;
    let blocks: u64 = windows
        .iter()
        .filter_map(|window| window["blocks"].as_u64())
        .sum();
    let mut lines = vec![
        version_info(),
        format!(
            "System: {} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        format!("FFmpeg: {}", ffmpeg_version()),
        format!("Sound: {}", AudioEngine::device_description(cx)),
        format!("Windows: {}, blocks: {blocks}", windows.len()),
    ];
    let changed = settings::changed(cx);
    if changed.is_empty() {
        lines.push("Settings: as Plays starts them out".to_owned());
    } else {
        lines.push("Settings changed:".to_owned());
        lines.extend(
            changed
                .iter()
                .map(|(key, value)| format!("  {key}: {value}")),
        );
    }
    Ok(lines.join("\n"))
}

/// The formats Plays opens, as `app.formats` hands them back.
fn formats() -> Value {
    file_types::FORMATS
        .iter()
        .map(|format| {
            json!({
                "name": format.name,
                "kind": format.kind.name(),
                "extensions": format.extensions,
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct Open {
    urls: Vec<String>,
    autoplay: Option<bool>,
    several_files: Option<SeveralFiles>,
    kind: Option<MediaKind>,
}

/// Video or sound, as `app.open`'s `kind` says it.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MediaKind {
    Video,
    Audio,
}

impl From<MediaKind> for file_types::Kind {
    fn from(kind: MediaKind) -> Self {
        match kind {
            MediaKind::Video => Self::Video,
            MediaKind::Audio => Self::Audio,
        }
    }
}

/// Open what `args` list in the window `target` names, or the one in
/// front, bringing it to the front, or in a new window with none open.
/// Hands back the window's id.
fn open(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let Open {
        urls,
        autoplay,
        several_files,
        kind,
    } = args.read()?;
    let sources = urls
        .iter()
        .map(|url| source(url).map_err(|why| Error::bad_args(args.command, why)))
        .collect::<Result<Vec<_>, _>>()?;
    let window = match target::window(target, cx) {
        Ok(window) => window,
        Err(_) if target.window.is_none() => Workspace::open_window(WINDOW_SIZE, cx)
            .map_err(|err| Error::failed(format!("can't open a window: {err:#}")))?,
        Err(err) => return Err(err),
    };
    window
        .update(cx, |workspace, window, cx| {
            window.activate_window();
            let opening = Opening {
                autoplay,
                several_files,
                kind: kind.map(Into::into),
            };
            workspace.open_as(sources, opening, cx);
        })
        .map_err(|_| Error::failed("the window has closed"))?;
    Ok(json!({"window": target::read(window, cx)?.id().to_string()}))
}

/// What `url` opens: a file or folder by its full path or file:// URL, or
/// a stream; or why it's neither.
fn source(url: &str) -> Result<Source, String> {
    if Path::new(url).is_absolute() {
        return Ok(Source::File(url.into()));
    }
    if let Some(source) = Source::from_open_url(url) {
        return Ok(source);
    }
    let why = match Source::parse_url(url) {
        Err(why) if url.contains("://") => why,
        _ => "give a file's full path, or a URL".to_owned(),
    };
    Err(format!("{url}: {why}"))
}

/// Show the panel `panel` picks over the video in Settings, and so in
/// every window and block, or hide it, as `switch` says, and keep it so;
/// with none, every panel.
fn switch_panel(switch: Switch, panel: Option<Panel>, cx: &mut App) {
    Settings::update(cx, true, |settings| {
        settings.over_video = settings.over_video.switched(panel, |on| switch.turn(on));
    });
}
