//! App-wide state, actions, key bindings, menus and windows.

use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Global, KeyBinding, Menu, MenuItem,
    SystemMenuType, TitlebarOptions, WindowBounds, WindowHandle, WindowOptions, actions, px, size,
};

use crate::hotkeys::*;
use crate::media::tracks::TrackKind;
use crate::media::{AudioOutput, Mixer, Source, picture};
use crate::settings::{SUBTITLE_POSITION_RANGE, SUBTITLE_SIZE_RANGE, Settings, SubtitleStyle};
use crate::ui::player_view::PlayerView;
use crate::ui::settings_view::SettingsView;
use crate::ui::theme::{UiFont, pick_ui_font, refresh_system_colors};
use crate::ui::workspace::Workspace;
use crate::ui::{TRAFFIC_LIGHTS, open_url, text_input};

// The playback ones, which can be rebound, are in `hotkeys`.
actions!(
    plays,
    [
        OpenFiles,
        OpenUrl,
        OpenSettings,
        Quit,
        CloseWindow,
        CloseAll
    ]
);

/// The shortcuts in the keymap, to tell when Settings change them.
struct BoundHotkeys(Hotkeys);
impl Global for BoundHotkeys {}

/// The single audio output shared by every player.
pub struct AudioEngine {
    output: Option<AudioOutput>,
    error: Option<String>,
}

impl Global for AudioEngine {}

impl AudioEngine {
    pub fn mixer(cx: &App) -> Option<Arc<Mixer>> {
        cx.try_global::<Self>()?
            .output
            .as_ref()
            .map(|o| o.mixer.clone())
    }

    pub fn device_description(cx: &App) -> String {
        match cx.try_global::<Self>() {
            Some(Self {
                output: Some(output),
                ..
            }) => format!(
                "{} · {} kHz",
                output.mixer.device_name,
                output.mixer.sample_rate as f32 / 1000.0
            ),
            Some(Self {
                error: Some(error), ..
            }) => format!("Unavailable: {error}"),
            _ => "Unavailable".into(),
        }
    }
}

struct SettingsWindow(Option<WindowHandle<SettingsView>>);
impl Global for SettingsWindow {}

/// `opened` brings the URLs the system asks the app to open.
pub fn run(cx: &mut App, sources: Vec<Source>, mut opened: UnboundedReceiver<Vec<String>>) {
    let settings = Settings::load();
    let [sdr_gamma, hdr_gamma] = settings.gamma();
    picture::set_gamma(sdr_gamma, hdr_gamma);
    let audio = match AudioOutput::new(settings.master_volume) {
        Ok(output) => AudioEngine {
            output: Some(output),
            error: None,
        },
        Err(err) => {
            log::error!("audio output unavailable, playing video only: {err:#}");
            AudioEngine {
                output: None,
                error: Some(err.to_string()),
            }
        }
    };
    cx.set_global(settings);
    cx.set_global(audio);
    let font = pick_ui_font(cx);
    cx.set_global(UiFont(font));
    refresh_system_colors(cx);
    cx.set_global(SettingsWindow(None));

    register_actions(cx);
    bind_keys(cx);
    watch_hotkeys(cx);

    let window =
        Workspace::open_window(size(px(1280.), px(800.)), cx).expect("opening the first window");

    if !sources.is_empty() {
        window
            .update(cx, |workspace, _, cx| workspace.open_sources(sources, cx))
            .ok();
    }

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
                with_workspace(cx, |workspace, window, cx| {
                    window.activate_window();
                    workspace.open_sources(sources, cx);
                })
            })
            .ok();
        }
    })
    .detach();

    // Closing the last workspace window quits, even if the settings window is
    // still open.
    cx.on_window_closed(|cx| {
        if first_workspace(cx.windows()).is_none() {
            cx.quit();
        }
    })
    .detach();
    cx.activate(true);
}

/// Run `f` on the workspace window in front, which menus, shortcuts and files
/// opened from Finder go to.
///
/// It runs once the current event is handled: gpui hands actions from menus
/// and keys to their listeners while it holds the window in front, and that
/// window can't be updated again until it lets go.
fn with_workspace(
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut gpui::Window, &mut gpui::Context<Workspace>) + 'static,
) {
    cx.defer(|cx| {
        let front = first_workspace(cx.active_window())
            .or_else(|| first_workspace(cx.window_stack().unwrap_or_default()))
            .or_else(|| first_workspace(cx.windows()));
        if let Some(handle) = front {
            handle
                .update(cx, |workspace, window, cx| f(workspace, window, cx))
                .inspect_err(|err| log::warn!("can't reach the window in front: {err:#}"))
                .ok();
        }
    });
}

fn first_workspace(
    windows: impl IntoIterator<Item = AnyWindowHandle>,
) -> Option<WindowHandle<Workspace>> {
    windows.into_iter().find_map(|w| w.downcast::<Workspace>())
}

fn register_actions(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    // Deferred, as in `with_workspace`, so the window in front (Settings,
    // say) can be reached.
    cx.on_action(|_: &OpenSettings, cx| cx.defer(open_settings));
    cx.on_action(|_: &CloseWindow, cx| {
        cx.defer(|cx| {
            if let Some(window) = cx.active_window() {
                window
                    .update(cx, |_, window, _| window.remove_window())
                    .inspect_err(|err| log::warn!("can't close the window: {err:#}"))
                    .ok();
            }
        })
    });
    cx.on_action(|_: &OpenFiles, cx| {
        with_workspace(cx, |workspace, window, cx| {
            window.activate_window();
            workspace.prompt_open(window, cx);
        })
    });
    cx.on_action(|_: &OpenUrl, cx| {
        with_workspace(cx, |workspace, window, cx| {
            window.activate_window();
            workspace.show_open_url(window, cx);
        })
    });
    cx.on_action(|_: &CloseAll, cx| with_workspace(cx, |w, _, cx| w.close_all(cx)));

    // The tile in front.
    cx.on_action(|_: &PlayPause, cx| with_active(cx, |p, cx| p.play_pause(cx)));
    cx.on_action(|_: &SeekForward, cx| {
        let step = seek_step(cx);
        with_active(cx, move |p, cx| p.seek_by(step, cx))
    });
    cx.on_action(|_: &SeekBackward, cx| {
        let step = seek_step(cx);
        with_active(cx, move |p, cx| p.seek_by(-step, cx))
    });
    cx.on_action(|_: &NextFrame, cx| with_active(cx, |p, cx| p.step_frame(1.0, cx)));
    cx.on_action(|_: &PreviousFrame, cx| with_active(cx, |p, cx| p.step_frame(-1.0, cx)));
    cx.on_action(|_: &Restart, cx| with_active(cx, |p, cx| p.restart(cx)));
    cx.on_action(|_: &ToggleLoop, cx| with_active(cx, |p, cx| p.toggle_loop(cx)));
    cx.on_action(|_: &VolumeUp, cx| with_active(cx, |p, cx| p.change_volume(0.05, cx)));
    cx.on_action(|_: &VolumeDown, cx| with_active(cx, |p, cx| p.change_volume(-0.05, cx)));
    cx.on_action(|_: &ToggleMute, cx| with_active(cx, |p, cx| p.toggle_mute(cx)));
    cx.on_action(|_: &NextAudioTrack, cx| {
        with_active(cx, |p, cx| p.next_track(TrackKind::Audio, cx))
    });
    cx.on_action(|_: &NextSubtitleTrack, cx| {
        with_active(cx, |p, cx| p.next_track(TrackKind::Subtitle, cx))
    });
    cx.on_action(|_: &NextVideoTrack, cx| {
        with_active(cx, |p, cx| p.next_track(TrackKind::Video, cx))
    });
    cx.on_action(|_: &ToggleSubtitles, cx| with_active(cx, |p, cx| p.toggle_subtitles(cx)));
    cx.on_action(|_: &BiggerSubtitles, cx| change_subtitle_size(cx, 0.1));
    cx.on_action(|_: &SmallerSubtitles, cx| change_subtitle_size(cx, -0.1));
    cx.on_action(|_: &SubtitlesUp, cx| change_subtitle_height(cx, 0.01));
    cx.on_action(|_: &SubtitlesDown, cx| change_subtitle_height(cx, -0.01));
    cx.on_action(|_: &ToggleFullScreen, cx| {
        with_workspace(cx, |_, window, _| window.toggle_fullscreen())
    });
    cx.on_action(|_: &ExitFullScreen, cx| {
        with_workspace(cx, |_, window, _| {
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        })
    });

    // Every tile in the window in front.
    cx.on_action(|_: &PlayPauseAll, cx| with_workspace(cx, |w, _, cx| w.toggle_all(cx)));
    cx.on_action(|_: &PlayAll, cx| with_workspace(cx, |w, _, cx| w.play_all(cx)));
    cx.on_action(|_: &PauseAll, cx| with_workspace(cx, |w, _, cx| w.pause_all(cx)));
    cx.on_action(|_: &RestartAll, cx| with_workspace(cx, |w, _, cx| w.restart_all(cx)));
    cx.on_action(|_: &SeekForwardAll, cx| with_workspace(cx, |w, _, cx| w.seek_all(1.0, cx)));
    cx.on_action(|_: &SeekBackwardAll, cx| with_workspace(cx, |w, _, cx| w.seek_all(-1.0, cx)));
    cx.on_action(|_: &ToggleMuteAll, cx| with_workspace(cx, |w, _, cx| w.toggle_mute_all(cx)));
}

/// Run `f` on the tile shortcuts go to in the window in front.
fn with_active(
    cx: &mut App,
    f: impl FnOnce(&mut PlayerView, &mut gpui::Context<PlayerView>) + 'static,
) {
    with_workspace(cx, |workspace, _, cx| workspace.with_active(cx, f));
}

fn seek_step(cx: &App) -> f64 {
    f64::from(cx.global::<Settings>().seek_step_seconds)
}

/// Change the subtitle style for every file, and say so on the tile in front.
fn change_subtitles(cx: &mut App, change: impl FnOnce(&mut SubtitleStyle) -> String) {
    let mut note = String::new();
    Settings::update(cx, true, |settings| {
        let mut style = settings.subtitles.clamped();
        note = change(&mut style);
        settings.subtitles = style;
    });
    with_active(cx, move |p, cx| p.note(note, cx));
}

fn change_subtitle_size(cx: &mut App, delta: f32) {
    change_subtitles(cx, |style| {
        let size = ((style.size + delta) * 20.0).round() / 20.0;
        style.size = size.clamp(*SUBTITLE_SIZE_RANGE.start(), *SUBTITLE_SIZE_RANGE.end());
        format!("Subtitle size {}%", (style.size * 100.0).round())
    });
}

fn change_subtitle_height(cx: &mut App, delta: f32) {
    change_subtitles(cx, |style| {
        let position = ((style.position + delta) * 100.0).round() / 100.0;
        style.position = position.clamp(
            *SUBTITLE_POSITION_RANGE.start(),
            *SUBTITLE_POSITION_RANGE.end(),
        );
        format!("Subtitle height {}%", (style.position * 100.0).round())
    });
}

/// Put the app's shortcuts and the playback ones from Settings in the
/// keymap, and show them in the menus.
fn bind_keys(cx: &mut App) {
    let hotkeys = cx.global::<Settings>().hotkeys.clone();
    cx.clear_key_bindings();
    text_input::bind_keys(cx);
    open_url::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("secondary-o", OpenFiles, None),
        KeyBinding::new("secondary-shift-o", OpenUrl, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-w", CloseWindow, None),
        KeyBinding::new("secondary-shift-w", CloseAll, Some("Workspace")),
    ]);
    cx.bind_keys(hotkeys.bindings());
    cx.set_global(BoundHotkeys(hotkeys));
    set_menus(cx);
}

/// Rebind when the shortcuts in Settings change.
fn watch_hotkeys(cx: &mut App) {
    cx.observe_global::<Settings>(|cx| {
        let changed = cx
            .try_global::<BoundHotkeys>()
            .is_none_or(|bound| bound.0 != cx.global::<Settings>().hotkeys);
        if changed {
            bind_keys(cx);
        }
    })
    .detach();
}

fn set_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "Plays".into(),
            items: vec![
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Quit Plays", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Open…", OpenFiles),
                MenuItem::action("Open URL…", OpenUrl),
                MenuItem::separator(),
                MenuItem::action("Close All Files", CloseAll),
                MenuItem::action("Close Window", CloseWindow),
            ],
        },
        Menu {
            name: "Playback".into(),
            items: vec![
                MenuItem::action("Play/Pause", PlayPause),
                MenuItem::action("Seek Forward", SeekForward),
                MenuItem::action("Seek Backward", SeekBackward),
                MenuItem::action("Next Frame", NextFrame),
                MenuItem::action("Previous Frame", PreviousFrame),
                MenuItem::action("Restart", Restart),
                MenuItem::action("Loop", ToggleLoop),
                MenuItem::separator(),
                MenuItem::action("Play/Pause All", PlayPauseAll),
                MenuItem::action("Play All", PlayAll),
                MenuItem::action("Pause All", PauseAll),
                MenuItem::action("Restart All", RestartAll),
                MenuItem::action("Seek All Forward", SeekForwardAll),
                MenuItem::action("Seek All Backward", SeekBackwardAll),
            ],
        },
        Menu {
            name: "Video".into(),
            items: vec![
                MenuItem::action("Full Screen", ToggleFullScreen),
                MenuItem::action("Next Video Track", NextVideoTrack),
            ],
        },
        Menu {
            name: "Audio".into(),
            items: vec![
                MenuItem::action("Volume Up", VolumeUp),
                MenuItem::action("Volume Down", VolumeDown),
                MenuItem::action("Mute/Unmute", ToggleMute),
                MenuItem::action("Next Audio Track", NextAudioTrack),
                MenuItem::separator(),
                MenuItem::action("Mute/Unmute All", ToggleMuteAll),
            ],
        },
        Menu {
            name: "Subtitles".into(),
            items: vec![
                MenuItem::action("Show/Hide Subtitles", ToggleSubtitles),
                MenuItem::action("Next Subtitles", NextSubtitleTrack),
                MenuItem::separator(),
                MenuItem::action("Bigger", BiggerSubtitles),
                MenuItem::action("Smaller", SmallerSubtitles),
                MenuItem::action("Move Up", SubtitlesUp),
                MenuItem::action("Move Down", SubtitlesDown),
            ],
        },
    ]);
}

pub fn open_settings(cx: &mut App) {
    if let Some(handle) = cx.global::<SettingsWindow>().0
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let bounds = Bounds::centered(None, size(px(720.), px(520.)), cx);
    // On macOS the sidebar runs up under the traffic lights, as in System
    // Settings.
    let mac = cfg!(target_os = "macos");
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Plays Settings".into()),
                    appears_transparent: mac,
                    traffic_light_position: mac.then_some(TRAFFIC_LIGHTS),
                }),
                window_min_size: Some(size(px(680.), px(420.))),
                app_id: Some("plays".into()),
                focus: true,
                ..Default::default()
            },
            |window, cx| cx.new(|cx| SettingsView::new(window, cx)),
        )
        .inspect_err(|err| log::error!("could not open settings: {err:#}"))
        .ok();
    cx.set_global(SettingsWindow(handle));
}
