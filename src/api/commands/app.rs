//! `app`: the app's own, and opening files and streams.

use std::path::Path;

use gpui::{App, ClipboardItem};
use serde::Deserialize;
use serde_json::json;

use super::{output, settings, window};
use crate::api::Error;
use crate::api::args::{Arg, Kind};
use crate::api::registry::{Args, Outcome, Scope, Spec};
use crate::api::target::{self, Target};
use crate::app::AudioEngine;
use crate::media::Source;
use crate::recents;
use crate::ui::about::{self, AboutView};
use crate::ui::settings::{SettingsView, ffmpeg_version};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

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
    Spec::custom_with(
        "app.open",
        "Open Paths or URLs",
        "Play files, the media files in folders, and streams, in the window named, or \
         the one in front, or a new one if none is open: each in a tile, or one after \
         another in one, as Settings say.",
        Scope::Window,
        &[Arg::required(
            "urls",
            Kind::Texts,
            "Each a file's or folder's full path, a file:// URL, or a stream's URL.",
        )],
        open,
    )
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
        "app.settings",
        "Settings…",
        "Open the Settings window, or bring it to the front.",
        SettingsView::open_window,
    )
    .keywords(&["preferences", "options", "shortcuts", "hotkeys"])
    .keys(&["secondary-,"]),
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
         where the sound goes, the windows and tiles, and the settings changed.",
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
        "app.quit",
        "Quit Plays",
        "Quit, keeping where each file got to and stopping the devices cast to.",
        |cx| cx.quit(),
    )
    .keywords(&["exit"])
    .keys(&["secondary-q"]),
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
/// many windows and tiles there are, and each setting changed.
fn debug_info(cx: &App) -> Result<String, Error> {
    let windows = window::summaries(cx)?;
    let tiles: u64 = windows
        .iter()
        .filter_map(|window| window["tiles"].as_u64())
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
        format!("Windows: {}, tiles: {tiles}", windows.len()),
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

#[derive(Deserialize)]
struct Open {
    urls: Vec<String>,
}

/// Open what `args` list in the window `target` names, or the one in
/// front, bringing it to the front, or in a new window with none open.
/// Hands back the window's id.
fn open(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let Open { urls } = args.read()?;
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
            workspace.open_sources(sources, cx);
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
