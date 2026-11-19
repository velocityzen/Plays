//! The menu bar. Its items show the keys bound to their actions, so the
//! keymap sets the menus again whenever it changes, and so does the audio
//! engine as outputs come and go or another is picked, Settings when
//! floating on top changes, and the displays as they come and go or the
//! window in front moves to another.

use std::time::Duration;

use gpui::{App, Global, Menu, MenuItem, SystemMenuType};

use super::dispatch::first_workspace;
use super::{
    About, AudioEngine, CloseAll, CloseFile, CloseWindow, OpenFiles, OpenSettings, OpenUrl, Quit,
    SetAspectRatio, SetAudioDevice, SetCrop, ShowOnDisplay,
};
use crate::hotkeys::*;
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, FloatOnTop, Settings};
use crate::ui::displays::{self, DisplayInfo};
use crate::ui::framing::{ASPECT_RATIOS, CROPS};

/// How often the displays are looked at: nothing tells when one connects,
/// as an AirPlay display does.
const DISPLAYS_POLL: Duration = Duration::from_secs(1);

pub(super) fn set_menus(cx: &mut App) {
    let outputs = output_menu(
        AudioEngine::devices(cx),
        cx.global::<Settings>().audio_output.device.as_ref(),
    );
    let float_on_top = cx.global::<Settings>().float_on_top;
    let displays = cx
        .try_global::<Displays>()
        .cloned()
        .unwrap_or_else(|| Displays::now(cx));
    cx.set_menus(vec![
        Menu {
            name: "Plays".into(),
            items: vec![
                MenuItem::action("About Plays", About),
                MenuItem::separator(),
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
                MenuItem::action("Close File", CloseFile),
                MenuItem::action("Close All Files", CloseAll),
                MenuItem::action("Close Window", CloseWindow),
            ],
        },
        Menu {
            name: "Playback".into(),
            items: vec![
                MenuItem::action("Play/Pause", PlayPause),
                MenuItem::action("Restart", Restart),
                MenuItem::action("Loop", ToggleLoop),
                MenuItem::separator(),
                MenuItem::action("Next Frame", NextFrame),
                MenuItem::action("Previous Frame", PreviousFrame),
                MenuItem::action("Seek Forward", SeekForward),
                MenuItem::action("Seek Backward", SeekBackward),
                MenuItem::action("Previous File", PreviousFile),
                MenuItem::action("Next File", NextFile),
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
                MenuItem::action("Picture in Picture", TogglePictureInPicture),
                MenuItem::action("Float on Top", ToggleFloatOnTop)
                    .checked(float_on_top == FloatOnTop::Always),
                MenuItem::action("Float on Top While Playing", ToggleFloatOnTopWhilePlaying)
                    .checked(float_on_top == FloatOnTop::WhilePlaying),
                MenuItem::submenu(display_menu(&displays)),
                MenuItem::separator(),
                MenuItem::action("Next Video Track", NextVideoTrack),
                MenuItem::separator(),
                MenuItem::action("Rotate Right", RotateRight),
                MenuItem::action("Rotate Left", RotateLeft),
                MenuItem::action("Flip Horizontally", FlipHorizontally),
                MenuItem::action("Flip Vertically", FlipVertically),
                MenuItem::separator(),
                MenuItem::submenu(Menu {
                    name: "Aspect Ratio".into(),
                    items: [MenuItem::action("As the File Is", SetAspectRatio(None))]
                        .into_iter()
                        .chain(
                            ASPECT_RATIOS
                                .map(|r| MenuItem::action(r.name, SetAspectRatio(Some(r)))),
                        )
                        .chain([
                            MenuItem::separator(),
                            MenuItem::action("Next Aspect Ratio", NextAspectRatio),
                        ])
                        .collect(),
                }),
                MenuItem::submenu(Menu {
                    name: "Crop".into(),
                    items: [MenuItem::action("None", SetCrop(None))]
                        .into_iter()
                        .chain(CROPS.map(|r| MenuItem::action(r.name, SetCrop(Some(r)))))
                        .chain([
                            MenuItem::separator(),
                            MenuItem::action("Next Crop", NextCrop),
                        ])
                        .collect(),
                }),
                MenuItem::separator(),
                MenuItem::action("Reset Picture", ResetPicture),
                MenuItem::separator(),
                MenuItem::action("Take Screenshot", TakeScreenshot),
                MenuItem::separator(),
                MenuItem::action("Console", ToggleConsole),
            ],
        },
        Menu {
            name: "Audio".into(),
            items: vec![
                MenuItem::action("Volume Up", VolumeUp),
                MenuItem::action("Volume Down", VolumeDown),
                MenuItem::action("Mute/Unmute", ToggleMute),
                MenuItem::separator(),
                MenuItem::action("Next Audio Track", NextAudioTrack),
                MenuItem::separator(),
                MenuItem::action("Audio Earlier", AudioEarlier),
                MenuItem::action("Audio Later", AudioLater),
                MenuItem::action("Reset Audio Delay", ResetAudioDelay),
                MenuItem::separator(),
                MenuItem::action("Mute/Unmute All", ToggleMuteAll),
                MenuItem::separator(),
                MenuItem::submenu(outputs),
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
                MenuItem::separator(),
                MenuItem::action("Earlier", SubtitlesEarlier),
                MenuItem::action("Later", SubtitlesLater),
                MenuItem::action("Reset Delay", ResetSubtitleDelay),
            ],
        },
    ]);
}

/// Set the menus again when floating on top, which they check, changes.
pub(super) fn watch_settings(cx: &mut App) {
    let mut shown = cx.global::<Settings>().float_on_top;
    cx.observe_global::<Settings>(move |cx| {
        let float_on_top = cx.global::<Settings>().float_on_top;
        if float_on_top != shown {
            shown = float_on_top;
            set_menus(cx);
        }
    })
    .detach();
}

/// What Video → Output Device shows: the displays, and the one the window
/// in front is on.
#[derive(Clone, Default, PartialEq, Debug)]
struct Displays {
    list: Vec<DisplayInfo>,
    front: Option<u32>,
}

impl Global for Displays {}

impl Displays {
    fn now(cx: &mut App) -> Self {
        let front = first_workspace(cx.active_window())
            .or_else(|| first_workspace(cx.window_stack().unwrap_or_default()))
            .or_else(|| first_workspace(cx.windows()))
            .and_then(|handle| {
                handle
                    .update(cx, |_, window, cx| displays::of(window, cx))
                    .ok()
                    .flatten()
            });
        Self {
            list: displays::list(cx),
            front,
        }
    }
}

/// Set the menus again when a display comes or goes, or the window in front
/// is on another.
pub(super) fn watch_displays(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(DISPLAYS_POLL).await;
            let alive = cx.update(|cx| {
                let now = Displays::now(cx);
                if cx.try_global::<Displays>() != Some(&now) {
                    cx.set_global(now);
                    set_menus(cx);
                }
            });
            if alive.is_err() {
                break;
            }
        }
    })
    .detach();
}

/// Every display, the one the window in front is on checked: picking one
/// shows the window there, full screen.
fn display_menu(displays: &Displays) -> Menu {
    Menu {
        name: "Output Device".into(),
        items: displays
            .list
            .iter()
            .map(|display| {
                MenuItem::action(display.name.clone(), ShowOnDisplay(display.id))
                    .checked(displays.front == Some(display.id))
            })
            .collect(),
    }
}

/// The system's default and every output connected, with the one picked
/// checked; one picked that isn't connected is listed too, as it is still
/// what the sound waits for.
fn output_menu(devices: &[DeviceInfo], picked: Option<&AudioDevice>) -> Menu {
    let mut items = vec![
        MenuItem::action("System Default", SetAudioDevice(None)).checked(picked.is_none()),
        MenuItem::separator(),
    ];
    items.extend(devices.iter().map(|device| {
        let checked = picked.is_some_and(|picked| picked.id == device.id);
        let device = AudioDevice {
            id: device.id.clone(),
            name: device.name.clone(),
        };
        MenuItem::action(device.name.clone(), SetAudioDevice(Some(device))).checked(checked)
    }));
    if let Some(picked) = picked
        && !devices.iter().any(|device| device.id == picked.id)
    {
        items.push(
            MenuItem::action(
                format!("{} (Not Connected)", picked.name),
                SetAudioDevice(Some(picked.clone())),
            )
            .checked(true),
        );
    }
    Menu {
        name: "Output Device".into(),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each action item's name and whether it is checked.
    fn listed(menu: &Menu) -> Vec<(String, bool)> {
        menu.items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action { name, checked, .. } => Some((name.to_string(), *checked)),
                _ => None,
            })
            .collect()
    }

    fn device(id: &str, name: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn checks_the_output_picked() {
        let devices = [device("a", "Speakers"), device("b", "Headphones")];
        assert_eq!(
            listed(&output_menu(&devices, None)),
            [
                ("System Default".into(), true),
                ("Speakers".into(), false),
                ("Headphones".into(), false),
            ]
        );
        let picked = AudioDevice {
            id: "b".into(),
            name: "Headphones".into(),
        };
        assert_eq!(
            listed(&output_menu(&devices, Some(&picked))),
            [
                ("System Default".into(), false),
                ("Speakers".into(), false),
                ("Headphones".into(), true),
            ]
        );
    }

    #[test]
    fn checks_the_display_the_window_in_front_is_on() {
        let display = |id, name: &str| DisplayInfo {
            id,
            name: name.into(),
        };
        let displays = Displays {
            list: vec![display(1, "Built-in Display"), display(7, "Beamer")],
            front: Some(7),
        };
        assert_eq!(
            listed(&display_menu(&displays)),
            [("Built-in Display".into(), false), ("Beamer".into(), true),]
        );
    }

    #[test]
    fn lists_the_output_picked_while_it_is_away() {
        let picked = AudioDevice {
            id: "c".into(),
            name: "TV".into(),
        };
        assert_eq!(
            listed(&output_menu(&[device("a", "Speakers")], Some(&picked))),
            [
                ("System Default".into(), false),
                ("Speakers".into(), false),
                ("TV (Not Connected)".into(), true),
            ]
        );
    }
}
