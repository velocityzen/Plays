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

use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{App, actions};

pub use audio::{AudioEngine, Route};
pub use awake::Awake;
pub use menus::refresh as refresh_menus;

use crate::cast::{self, Casting, Device, Protocol};
use crate::media::{Source, picture};
use crate::settings::{AudioDevice, Settings};
use crate::ui::framing::Ratio;
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::theme::{UiFont, pick_ui_font, refresh_system_colors};
use crate::ui::workspace::Workspace;

/// How often quitting looks whether the devices cast to have stopped.
const STOP_POLL: Duration = Duration::from_millis(5);

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

/// Show the window in front full screen on this display (its id).
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = plays, no_json)]
pub struct ShowOnDisplay(pub u32);

/// Cast the tile in front to this device, or play it on this computer.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = plays, no_json)]
pub struct CastTo(pub Option<Device>);

/// `opened` brings the URLs the system asks the app to open.
pub fn run(cx: &mut App, sources: Vec<Source>, opened: UnboundedReceiver<Vec<String>>) {
    set_up(cx, Settings::load(), AudioEngine::start, cast::protocols);
    awake::watch(cx);

    let window = Workspace::open_window(WINDOW_SIZE, cx).expect("opening the first window");

    if !sources.is_empty() {
        window
            .update(cx, |workspace, _, cx| workspace.open_sources(sources, cx))
            .ok();
    }
    open_from_system(cx, opened);

    stop_casts_on_quit(cx);

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
/// starts, casting with `protocols` (those Settings have on), the UI font
/// and colors, and the actions and their keys.
fn set_up(cx: &mut App, settings: Settings, audio: fn(&mut App), protocols: fn() -> Vec<Protocol>) {
    let [sdr_gamma, hdr_gamma] = settings.gamma();
    picture::set_gamma(sdr_gamma, hdr_gamma);
    cx.set_global(settings);
    audio(cx);
    cx.set_global(Casting::new(protocols()));
    Casting::watch(cx);
    let font = pick_ui_font(cx);
    cx.set_global(UiFont(font));
    refresh_system_colors(cx);

    commands::register(cx);
    keymap::bind_keys(cx);
    keymap::watch_hotkeys(cx);
    menus::watch_settings(cx);
    menus::watch_outputs(cx);
}

/// For `#[gpui::test]`s: the app as it starts, with default settings
/// (never saved), silent audio, no devices to cast to, and without keeping
/// the computer awake.
#[cfg(test)]
pub fn set_up_for_tests(cx: &mut App) {
    set_up(cx, Settings::default(), AudioEngine::start_silent, Vec::new);
}

/// Devices cast to stop as the app quits, rather than wait on a stream
/// server that's gone; the app waits a moment for them.
fn stop_casts_on_quit(cx: &mut App) {
    cx.on_app_quit(|cx| {
        let stopping: Vec<_> = cx
            .windows()
            .into_iter()
            .filter_map(|window| window.downcast::<Workspace>())
            .filter_map(|window| window.update(cx, |w, _, cx| w.stop_casts(cx)).ok())
            .flatten()
            .collect();
        let executor = cx.background_executor().clone();
        async move {
            while !stopping.iter().all(|thread| thread.is_finished()) {
                executor.timer(STOP_POLL).await;
            }
        }
    })
    .detach();
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
