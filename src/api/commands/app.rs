//! `app`: the app's own, and opening files and streams.

use std::path::Path;

use gpui::App;
use serde::Deserialize;
use serde_json::json;

use super::{output, window};
use crate::api::Error;
use crate::api::args::{Arg, Kind};
use crate::api::registry::{Args, Outcome, Scope, Spec};
use crate::api::target::{self, Target};
use crate::media::Source;
use crate::ui::about::AboutView;
use crate::ui::settings::SettingsView;
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
         the one in front, or a new one if none is open.",
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
    Spec::app(
        "app.quit",
        "Quit Plays",
        "Quit, keeping where each file got to and stopping the devices cast to.",
        |cx| cx.quit(),
    )
    .keywords(&["exit"])
    .keys(&["secondary-q"]),
];

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
