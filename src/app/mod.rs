//! The app: starting it. Its commands are the API's (`crate::api`), whose
//! keys are bound in `keymap` and which the menu bar (`menus`) runs; going
//! from window to window is in `windows`.

mod audio;
mod awake;
mod bluetooth;
mod history;
mod keymap;
mod menus;
#[cfg(feature = "smoke")]
pub mod smoke;
mod speakers;
mod windows;

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::App;
use serde_json::json;

pub use audio::{AudioEngine, Route};
#[cfg(test)]
pub(crate) use audio::{Bluetooth, Devices, Local};
pub use awake::Awake;
#[cfg(test)]
pub(crate) use bluetooth::Paired;
pub(crate) use history::FocusHistory;
pub use menus::refresh as refresh_menus;
#[cfg(test)]
pub(crate) use speakers::Speakers;
pub use speakers::{airplay_id, output_kind};
pub(crate) use windows::{
    back as back_window, move_to as move_window, note_front, players as player_windows,
    recent as recent_window, step as step_window,
};

use crate::api::{self, Invocation};
use crate::cast::{self, AppPairings, Casting, Pairings, Protocol};
use crate::files::Folder;
use crate::media::{Source, picture};
use crate::progress::{self, Progress};
use crate::settings::Settings;
use crate::ui::theme::{UiFont, pick_ui_font, refresh_system_colors};
use crate::ui::workspace::{Workspace, follow_over_video_settings, save_place_on_quit};
use crate::ui::{blackout, displays};

/// How often quitting looks whether the devices cast to have stopped.
const STOP_POLL: Duration = Duration::from_millis(5);

/// `opened` brings the URLs the system asks the app to open; `listener`,
/// if it has one, the command line's and other Plays' requests.
pub fn run(
    cx: &mut App,
    sources: Vec<Source>,
    opened: UnboundedReceiver<Vec<String>>,
    listener: Option<api::socket::Listener>,
) {
    let folder = Folder::app();
    cx.set_global(folder.clone());
    set_up(
        cx,
        Settings::load(&folder),
        AudioEngine::start,
        cast::protocols,
    );
    awake::watch(cx);
    blackout::watch(cx);
    displays::watch(cx);

    let window = Workspace::open_first_window(cx).expect("opening the first window");
    save_place_on_quit(cx);

    if !sources.is_empty() {
        window
            .update(cx, |workspace, _, cx| workspace.open_sources(sources, cx))
            .ok();
    }
    open_from_system(cx, opened);
    if let Some(listener) = listener {
        api::socket::serve(listener, cx);
    }

    // Progress is noted as the app quits before casts stop, while each
    // knows where its device is.
    keep_progress(cx);
    stop_casts_on_quit(cx);

    // Closing the last workspace window quits, even if the settings window is
    // still open.
    cx.on_window_closed(|cx| {
        if api::front_window(cx).is_none() {
            cx.quit();
        }
    })
    .detach();
    cx.activate(true);
}

/// What every window needs: `settings`, the audio engine, which `audio`
/// starts, casting with `protocols` (those Settings have on), the UI font
/// and colors, and the commands, their keys and the menus.
fn set_up(
    cx: &mut App,
    settings: Settings,
    audio: fn(&mut App),
    protocols: fn(Arc<Pairings>) -> Vec<Protocol>,
) {
    let [sdr_gamma, hdr_gamma] = settings.gamma();
    picture::set_gamma(sdr_gamma, hdr_gamma);
    cx.set_global(settings);
    let folder = Folder::of(cx);
    cx.set_global(Progress::load(&folder));
    // Speakers are paired with as TVs are.
    let pairings = Arc::new(Pairings::load(&folder));
    cx.set_global(AppPairings(Arc::clone(&pairings)));
    audio(cx);
    cx.set_global(Casting::new(protocols(pairings)));
    Casting::watch(cx);
    let font = pick_ui_font(cx);
    cx.set_global(UiFont(font));
    refresh_system_colors(cx);

    windows::number(cx);
    crate::api::register(cx);
    keymap::bind_keys(cx);
    keymap::watch_hotkeys(cx);
    menus::watch_settings(cx);
    menus::watch_outputs(cx);
    follow_over_video_settings(cx);
}

/// For `#[gpui::test]`s: the app as it starts, with default settings
/// (never saved), silent audio, no devices to cast to, and without keeping
/// the computer awake.
#[cfg(test)]
pub fn set_up_for_tests(cx: &mut App) {
    set_up(cx, Settings::default(), AudioEngine::start_silent, |_| {
        Vec::new()
    });
}

/// Note where every open file is, and save it, every
/// `progress::SAVE_EVERY` and as the app quits.
fn keep_progress(cx: &mut App) {
    fn note_all(cx: &mut App) {
        let notes: Vec<progress::Note> = cx
            .windows()
            .into_iter()
            .filter_map(|window| window.downcast::<Workspace>())
            .filter_map(|window| window.read(cx).ok().map(|w| w.progress(cx)))
            .flatten()
            .collect();
        progress::note(notes, true, cx);
    }
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(progress::SAVE_EVERY).await;
            if cx.update(note_all).is_err() {
                break;
            }
        }
    })
    .detach();
    cx.on_app_quit(|cx| {
        note_all(cx);
        async {}
    })
    .detach();
}

/// Devices cast to stop as the app quits, rather than wait on a stream
/// server that's gone; the app waits a moment for them.
fn stop_casts_on_quit(cx: &mut App) {
    cx.on_app_quit(|cx| {
        let stopping = stop_all_casts(cx);
        let executor = cx.background_executor().clone();
        async move {
            while !stopping.iter().all(|thread| thread.is_finished()) {
                executor.timer(STOP_POLL).await;
            }
        }
    })
    .detach();
}

/// Stop every device cast to from the windows open, handing back the
/// threads stopping them and those stopping devices cast to from windows
/// closed before.
pub(crate) fn stop_all_casts(cx: &mut App) -> Vec<std::thread::JoinHandle<()>> {
    let mut stopping: Vec<_> = cx
        .windows()
        .into_iter()
        .filter_map(|window| window.downcast::<Workspace>())
        .filter_map(|window| window.update(cx, |w, _, cx| w.stop_casts(cx)).ok())
        .flatten()
        .collect();
    stopping.extend(cast::Stopping::take(cx));
    stopping
}

/// Open what the system sends, files from Finder and links, in the
/// workspace window in front (`app.open`).
fn open_from_system(cx: &mut App, mut opened: UnboundedReceiver<Vec<String>>) {
    cx.spawn(async move |cx| {
        while let Some(urls) = opened.next().await {
            let urls: Vec<String> = urls
                .into_iter()
                .filter(|url| {
                    let opens = Source::from_open_url(url).is_some();
                    if !opens {
                        log::warn!("can't open a URL the system sent");
                    }
                    opens
                })
                .collect();
            if urls.is_empty() {
                continue;
            }
            let open = Invocation::new("app.open").with_args(json!({"urls": urls}));
            cx.update(|cx| api::dispatch(open, cx)).ok();
        }
    })
    .detach();
}
