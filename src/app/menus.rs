//! The menu bar. Its items show the keys bound to their actions, so the
//! keymap sets the menus again whenever it changes, and so does the audio
//! engine as outputs come and go or another is picked, and Settings when
//! floating on top changes.

use gpui::{App, Menu, MenuItem, SystemMenuType};

use super::{
    About, AudioEngine, CloseAll, CloseFile, CloseWindow, OpenFiles, OpenSettings, OpenUrl, Quit,
    SetAspectRatio, SetAudioDevice, SetCrop,
};
use crate::hotkeys::*;
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, FloatOnTop, Settings};
use crate::ui::framing::{ASPECT_RATIOS, CROPS};

pub(super) fn set_menus(cx: &mut App) {
    let outputs = output_menu(
        AudioEngine::devices(cx),
        cx.global::<Settings>().audio_output.device.as_ref(),
    );
    let float_on_top = cx.global::<Settings>().float_on_top;
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
                MenuItem::action("Seek Forward", SeekForward),
                MenuItem::action("Seek Backward", SeekBackward),
                MenuItem::action("Next Frame", NextFrame),
                MenuItem::action("Previous Frame", PreviousFrame),
                MenuItem::action("Restart", Restart),
                MenuItem::action("Previous File", PreviousFile),
                MenuItem::action("Next File", NextFile),
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
                MenuItem::action("Picture in Picture", TogglePictureInPicture),
                MenuItem::action("Float on Top", ToggleFloatOnTop)
                    .checked(float_on_top == FloatOnTop::Always),
                MenuItem::action("Float on Top While Playing", ToggleFloatOnTopWhilePlaying)
                    .checked(float_on_top == FloatOnTop::WhilePlaying),
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
                MenuItem::action("Console", ToggleConsole),
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
