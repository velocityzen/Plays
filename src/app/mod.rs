//! The app: starting it, and its own actions. What actions do is in
//! `commands`, how they reach the window and tile in front in `dispatch`,
//! their keys in `keymap` and the menu bar in `menus`.

mod audio;
mod awake;
mod commands;
mod dispatch;
mod keymap;
mod menus;
#[cfg(feature = "smoke")]
pub mod smoke;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{App, actions};

pub use audio::{AudioEngine, Route};
pub use awake::Awake;

use crate::media::{Source, picture};
use crate::settings::{AudioDevice, Settings};
use crate::ui::framing::Ratio;
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::theme::{UiFont, pick_ui_font, refresh_system_colors};
use crate::ui::workspace::Workspace;

// The playback ones, which can be rebound, are in `hotkeys`.
actions!(
    plays,
    [
        About,
        OpenFiles,
        OpenUrl,
        OpenSettings,
        Quit,
        CloseFile,
        CloseWindow,
        CloseAll
    ]
);

/// Show the picture in front at this aspect ratio, or its own.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = plays, no_json)]
pub struct SetAspectRatio(pub Option<Ratio>);

/// Crop the picture in front to this shape, or not at all.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = plays, no_json)]
pub struct SetCrop(pub Option<Ratio>);

/// Play through this output device, or the system's default.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = plays, no_json)]
pub struct SetAudioDevice(pub Option<AudioDevice>);

/// `opened` brings the URLs the system asks the app to open.
pub fn run(cx: &mut App, sources: Vec<Source>, opened: UnboundedReceiver<Vec<String>>) {
    set_up(cx, Settings::load(), AudioEngine::start);
    awake::watch(cx);

    let window = Workspace::open_window(WINDOW_SIZE, cx).expect("opening the first window");

    if !sources.is_empty() {
        window
            .update(cx, |workspace, _, cx| workspace.open_sources(sources, cx))
            .ok();
    }
    open_from_system(cx, opened);

    // Closing the last workspace window quits, even if the settings window is
    // still open.
    cx.on_window_closed(|cx| {
        if dispatch::first_workspace(cx.windows()).is_none() {
            cx.quit();
        }
    })
    .detach();
    cx.activate(true);
}

/// What every window needs: `settings`, the audio engine, which `audio`
/// starts, the UI font and colors, and the actions and their keys.
fn set_up(cx: &mut App, settings: Settings, audio: fn(&mut App)) {
    let [sdr_gamma, hdr_gamma] = settings.gamma();
    picture::set_gamma(sdr_gamma, hdr_gamma);
    cx.set_global(settings);
    audio(cx);
    let font = pick_ui_font(cx);
    cx.set_global(UiFont(font));
    refresh_system_colors(cx);

    commands::register(cx);
    keymap::bind_keys(cx);
    keymap::watch_hotkeys(cx);
    menus::watch_settings(cx);
}

/// For `#[gpui::test]`s: the app as it starts, with default settings
/// (never saved), silent audio, and without keeping the computer awake.
#[cfg(test)]
pub fn set_up_for_tests(cx: &mut App) {
    set_up(cx, Settings::default(), AudioEngine::start_silent);
}

/// Open what the system sends, files from Finder and links, in the
/// workspace window in front.
fn open_from_system(cx: &mut App, mut opened: UnboundedReceiver<Vec<String>>) {
    cx.spawn(async move |cx| {
        while let Some(urls) = opened.next().await {
            let sources: Vec<Source> = urls
                .iter()
                .filter_map(|url| {
                    let source = Source::from_open_url(url);
                    if source.is_none() {
                        log::warn!("can't open a URL the system sent");
                    }
                    source
                })
                .collect();
            if sources.is_empty() {
                continue;
            }
            cx.update(|cx| {
                dispatch::with_workspace(cx, |workspace, window, cx| {
                    window.activate_window();
                    workspace.open_sources(sources, cx);
                })
            })
            .ok();
        }
    })
    .detach();
}
