//! The menu bar. Its items show the keys bound to their actions, so the
//! keymap sets the menus again whenever it changes.

use gpui::{App, Menu, MenuItem, SystemMenuType};

use super::{
    CloseAll, CloseWindow, OpenFiles, OpenSettings, OpenUrl, Quit, SetAspectRatio, SetCrop,
};
use crate::hotkeys::*;
use crate::ui::framing::{ASPECT_RATIOS, CROPS};

pub(super) fn set_menus(cx: &mut App) {
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
