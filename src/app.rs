//! App-wide state, actions, key bindings, menus and windows.

use std::sync::Arc;

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Global, KeyBinding, Menu, MenuItem,
    SystemMenuType, TitlebarOptions, WindowBounds, WindowHandle, WindowOptions, actions, px, size,
};

use crate::media::{AudioOutput, Mixer, Source};
use crate::settings::Settings;
use crate::ui::settings_view::SettingsView;
use crate::ui::theme::{UiFont, pick_ui_font, refresh_system_colors};
use crate::ui::workspace::Workspace;
use crate::ui::{open_url, text_input};

actions!(
    plays,
    [
        OpenFiles,
        OpenUrl,
        OpenSettings,
        Quit,
        CloseWindow,
        CloseAll,
        PlayPauseAll,
        PlayAll,
        PauseAll,
        RestartAll,
        SeekForward,
        SeekBackward,
        ToggleMuteAll,
    ]
);

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
    set_menus(cx);

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
fn with_workspace(
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut gpui::Window, &mut gpui::Context<Workspace>),
) {
    let front = first_workspace(cx.active_window())
        .or_else(|| first_workspace(cx.window_stack().unwrap_or_default()))
        .or_else(|| first_workspace(cx.windows()));
    if let Some(handle) = front {
        handle
            .update(cx, |workspace, window, cx| f(workspace, window, cx))
            .ok();
    }
}

fn first_workspace(
    windows: impl IntoIterator<Item = AnyWindowHandle>,
) -> Option<WindowHandle<Workspace>> {
    windows.into_iter().find_map(|w| w.downcast::<Workspace>())
}

fn register_actions(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &OpenSettings, cx| open_settings(cx));
    cx.on_action(|_: &CloseWindow, cx| {
        if let Some(window) = cx.active_window() {
            window
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
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
    cx.on_action(|_: &PlayPauseAll, cx| with_workspace(cx, |w, _, cx| w.toggle_all(cx)));
    cx.on_action(|_: &PlayAll, cx| with_workspace(cx, |w, _, cx| w.play_all(cx)));
    cx.on_action(|_: &PauseAll, cx| with_workspace(cx, |w, _, cx| w.pause_all(cx)));
    cx.on_action(|_: &RestartAll, cx| with_workspace(cx, |w, _, cx| w.restart_all(cx)));
    cx.on_action(|_: &SeekForward, cx| with_workspace(cx, |w, _, cx| w.seek_all(1.0, cx)));
    cx.on_action(|_: &SeekBackward, cx| with_workspace(cx, |w, _, cx| w.seek_all(-1.0, cx)));
    cx.on_action(|_: &ToggleMuteAll, cx| with_workspace(cx, |w, _, cx| w.toggle_mute_all(cx)));
}

fn bind_keys(cx: &mut App) {
    text_input::bind_keys(cx);
    open_url::bind_keys(cx);
    cx.bind_keys([
        KeyBinding::new("secondary-o", OpenFiles, None),
        KeyBinding::new("secondary-shift-o", OpenUrl, None),
        KeyBinding::new("secondary-,", OpenSettings, None),
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("secondary-w", CloseWindow, None),
        KeyBinding::new("secondary-shift-w", CloseAll, Some("Workspace")),
        KeyBinding::new("space", PlayPauseAll, Some("Workspace")),
        KeyBinding::new("k", PlayPauseAll, Some("Workspace")),
        KeyBinding::new("right", SeekForward, Some("Workspace")),
        KeyBinding::new("l", SeekForward, Some("Workspace")),
        KeyBinding::new("left", SeekBackward, Some("Workspace")),
        KeyBinding::new("j", SeekBackward, Some("Workspace")),
        KeyBinding::new("m", ToggleMuteAll, Some("Workspace")),
        KeyBinding::new("home", RestartAll, Some("Workspace")),
    ]);
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
                MenuItem::action("Play/Pause All", PlayPauseAll),
                MenuItem::action("Play All", PlayAll),
                MenuItem::action("Pause All", PauseAll),
                MenuItem::action("Restart All", RestartAll),
                MenuItem::separator(),
                MenuItem::action("Seek Forward", SeekForward),
                MenuItem::action("Seek Backward", SeekBackward),
                MenuItem::separator(),
                MenuItem::action("Mute/Unmute All", ToggleMuteAll),
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
    let bounds = Bounds::centered(None, size(px(560.), px(680.)), cx);
    let handle = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Plays Settings".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(440.), px(360.))),
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
