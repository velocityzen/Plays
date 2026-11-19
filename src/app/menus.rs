//! The menu bar. Its items show the keys bound to their actions, so the
//! keymap sets the menus again whenever it changes, and so does the audio
//! engine as outputs come and go or another is picked, Settings when
//! floating on top changes, the displays as they come and go or the window
//! in front moves to another, and the devices to cast to as they come and
//! go or the tile in front casts to one.

use std::time::Duration;

use gpui::{App, Global, Menu, MenuItem, SystemMenuType};

use super::dispatch::first_workspace;
use super::{
    About, AudioEngine, CastTo, CloseAll, CloseFile, CloseWindow, OpenFiles, OpenSettings, OpenUrl,
    Quit, SetAspectRatio, SetAudioDevice, SetCrop, ShowOnDisplay,
};
use crate::cast::{Casting, Device, Shows};
use crate::hotkeys::*;
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, FloatOnTop, Settings};
use crate::ui::displays::{self, DisplayInfo};
use crate::ui::framing::{ASPECT_RATIOS, CROPS};

/// How often the displays, and the tile in front's cast, are looked at:
/// nothing tells when a display connects, as an AirPlay display does.
const DISPLAYS_POLL: Duration = Duration::from_secs(1);

pub(super) fn set_menus(cx: &mut App) {
    let float_on_top = cx.global::<Settings>().float_on_top;
    let video_outputs = cx
        .try_global::<Outputs>()
        .cloned()
        .unwrap_or_else(|| Outputs::now(cx));
    let outputs = output_menu(
        AudioEngine::devices(cx),
        cx.global::<Settings>().audio_output.device.as_ref(),
        &video_outputs,
    );
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
                MenuItem::submenu(display_menu(&video_outputs)),
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

/// What Video → Output Device and Audio → Output Device show besides this
/// computer's outputs: the displays, the one the window in front is on, the
/// devices that show video, the speakers, and the one the tile in front
/// casts to.
#[derive(Clone, Default, PartialEq, Debug)]
struct Outputs {
    displays: Vec<DisplayInfo>,
    front: Option<u32>,
    devices: Vec<Device>,
    /// Those for the tile in front: none when it has no sound.
    speakers: Vec<Device>,
    casting: Option<Device>,
}

impl Global for Outputs {}

impl Outputs {
    fn now(cx: &mut App) -> Self {
        let window = first_workspace(cx.active_window())
            .or_else(|| first_workspace(cx.window_stack().unwrap_or_default()))
            .or_else(|| first_workspace(cx.windows()));
        let (front, casting, has_audio) = window
            .and_then(|handle| {
                handle
                    .update(cx, |workspace, window, cx| {
                        (
                            displays::of(window, cx),
                            workspace.active_casting(cx),
                            workspace.active_has_audio(cx),
                        )
                    })
                    .ok()
            })
            .unwrap_or((None, None, true));
        let (devices, speakers) = Casting::devices_for(cx, has_audio)
            .into_iter()
            .partition(|device| device.shows == Shows::Video);
        Self {
            displays: displays::list(cx),
            front,
            devices,
            speakers,
            casting,
        }
    }
}

/// Set the menus again when a display or a device comes or goes, the
/// window in front is on another display, or the tile in front casts.
pub(super) fn watch_outputs(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(DISPLAYS_POLL).await;
            if cx.update(refresh_outputs).is_err() {
                break;
            }
        }
    })
    .detach();
}

fn refresh_outputs(cx: &mut App) {
    let now = Outputs::now(cx);
    if cx.try_global::<Outputs>() != Some(&now) {
        cx.set_global(now);
        set_menus(cx);
    }
}

/// Every display, the one the window in front is on checked, then the
/// devices that show video, the one the tile in front casts to checked
/// instead (a speaker it casts to leaves its picture here). Picking a
/// display shows the window there, full screen; picking a device casts the
/// tile in front to it.
fn display_menu(outputs: &Outputs) -> Menu {
    let casting = outputs
        .casting
        .as_ref()
        .filter(|device| device.shows == Shows::Video);
    let mut items: Vec<MenuItem> = outputs
        .displays
        .iter()
        .map(|display| {
            MenuItem::action(display.name.clone(), ShowOnDisplay(display.id))
                .checked(casting.is_none() && outputs.front == Some(display.id))
        })
        .collect();
    let mut devices = outputs.devices.clone();
    // One that is cast to stays listed while it isn't found.
    if let Some(casting) = casting
        && !devices.iter().any(|device| device.is(casting))
    {
        devices.push(casting.clone());
    }
    if !devices.is_empty() {
        items.push(MenuItem::separator());
    }
    items.extend(devices.into_iter().map(|device| {
        let checked = casting.is_some_and(|casting| casting.is(&device));
        MenuItem::action(device.name.clone(), CastTo(Some(device))).checked(checked)
    }));
    Menu {
        name: "Output Device".into(),
        items,
    }
}

/// The system's default and every output connected, with the one picked
/// checked; one picked that isn't connected is listed too, as it is still
/// what the sound waits for. Then the speakers, and the device the tile in
/// front casts to, checked instead: picking one casts the tile in front to
/// it.
fn output_menu(devices: &[DeviceInfo], picked: Option<&AudioDevice>, outputs: &Outputs) -> Menu {
    let casting = outputs.casting.as_ref();
    let here = casting.is_none();
    let mut items = vec![
        MenuItem::action("System Default", SetAudioDevice(None)).checked(here && picked.is_none()),
        MenuItem::separator(),
    ];
    items.extend(devices.iter().map(|device| {
        let checked = here && picked.is_some_and(|picked| picked.id == device.id);
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
            .checked(here),
        );
    }
    let mut speakers = outputs.speakers.clone();
    // What it casts to plays its sound, and stays listed while it isn't
    // found.
    if let Some(casting) = casting
        && !speakers.iter().any(|device| device.is(casting))
    {
        speakers.push(casting.clone());
    }
    if !speakers.is_empty() {
        items.push(MenuItem::separator());
    }
    items.extend(speakers.into_iter().map(|device| {
        let checked = casting.is_some_and(|casting| casting.is(&device));
        MenuItem::action(device.name.clone(), CastTo(Some(device))).checked(checked)
    }));
    Menu {
        name: "Output Device".into(),
        items,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::cast::fake::FakeTarget;
    use crate::ui::testing::{self, OPEN_WAIT};

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
            listed(&output_menu(&devices, None, &Outputs::default())),
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
            listed(&output_menu(&devices, Some(&picked), &Outputs::default())),
            [
                ("System Default".into(), false),
                ("Speakers".into(), false),
                ("Headphones".into(), true),
            ]
        );
    }

    fn display(id: u32, name: &str) -> DisplayInfo {
        DisplayInfo {
            id,
            name: name.into(),
        }
    }

    fn tv(id: &str, name: &str) -> Device {
        Device {
            kind: "Fake",
            id: id.into(),
            name: name.into(),
            shows: Shows::Video,
            address: std::net::Ipv4Addr::LOCALHOST.into(),
        }
    }

    fn speaker(id: &str, name: &str) -> Device {
        Device {
            shows: Shows::Sound,
            ..tv(id, name)
        }
    }

    #[test]
    fn lists_the_speakers_after_the_outputs_and_checks_the_one_cast_to_instead() {
        let devices = [device("a", "Headphones")];
        let mut outputs = Outputs {
            speakers: vec![speaker("k", "Kitchen"), speaker("p", "Phantom")],
            ..Outputs::default()
        };
        assert_eq!(
            listed(&output_menu(&devices, None, &outputs)),
            [
                ("System Default".into(), true),
                ("Headphones".into(), false),
                ("Kitchen".into(), false),
                ("Phantom".into(), false),
            ]
        );
        outputs.casting = Some(speaker("p", "Phantom"));
        assert_eq!(
            listed(&output_menu(&devices, None, &outputs)),
            [
                ("System Default".into(), false),
                ("Headphones".into(), false),
                ("Kitchen".into(), false),
                ("Phantom".into(), true),
            ]
        );
        // A TV it casts to plays its sound too.
        outputs.casting = Some(tv("t", "Living Room"));
        assert_eq!(
            listed(&output_menu(&devices, None, &outputs))
                .last()
                .cloned(),
            Some(("Living Room".into(), true))
        );
    }

    #[test]
    fn a_speaker_cast_to_leaves_the_picture_on_its_display() {
        let outputs = Outputs {
            displays: vec![display(1, "Built-in Display")],
            front: Some(1),
            devices: vec![tv("t", "Living Room")],
            speakers: vec![speaker("k", "Kitchen")],
            casting: Some(speaker("k", "Kitchen")),
        };
        assert_eq!(
            listed(&display_menu(&outputs)),
            [
                ("Built-in Display".into(), true),
                ("Living Room".into(), false),
            ]
        );
    }

    #[test]
    fn checks_the_display_the_window_in_front_is_on() {
        let outputs = Outputs {
            displays: vec![display(1, "Built-in Display"), display(7, "Beamer")],
            front: Some(7),
            ..Outputs::default()
        };
        assert_eq!(
            listed(&display_menu(&outputs)),
            [("Built-in Display".into(), false), ("Beamer".into(), true),]
        );
    }

    #[test]
    fn checks_the_device_the_tile_in_front_casts_to_instead() {
        let mut outputs = Outputs {
            displays: vec![display(1, "Built-in Display")],
            front: Some(1),
            devices: vec![tv("a", "Bedroom"), tv("b", "Living Room")],
            ..Outputs::default()
        };
        assert_eq!(
            listed(&display_menu(&outputs)),
            [
                ("Built-in Display".into(), true),
                ("Bedroom".into(), false),
                ("Living Room".into(), false),
            ]
        );
        outputs.casting = Some(tv("b", "Living Room"));
        assert_eq!(
            listed(&display_menu(&outputs)),
            [
                ("Built-in Display".into(), false),
                ("Bedroom".into(), false),
                ("Living Room".into(), true),
            ]
        );
        // Still cast to while it isn't found.
        outputs.devices.pop();
        assert_eq!(
            listed(&display_menu(&outputs)).last().cloned(),
            Some(("Living Room".into(), true))
        );
    }

    #[test]
    fn lists_the_output_picked_while_it_is_away() {
        let picked = AudioDevice {
            id: "c".into(),
            name: "TV".into(),
        };
        assert_eq!(
            listed(&output_menu(
                &[device("a", "Speakers")],
                Some(&picked),
                &Outputs::default()
            )),
            [
                ("System Default".into(), false),
                ("Speakers".into(), false),
                ("TV (Not Connected)".into(), true),
            ]
        );
    }

    #[gpui::test]
    fn lists_the_devices_with_video_and_checks_the_one_cast_to(cx: &mut TestAppContext) {
        let (window, mut cx) = testing::start(cx);
        let fake = FakeTarget::default();
        let tv = fake.add("Living Room", Shows::Video);
        fake.add("Kitchen", Shows::Sound);
        cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
        window
            .update(&mut cx, |workspace, _, cx| {
                workspace.open_paths(vec![testing::fixture("clip.mp4")], cx)
            })
            .unwrap();
        let start = Instant::now();
        while !testing::shows(&mut cx, "tile-0") {
            assert!(start.elapsed() < OPEN_WAIT, "clip.mp4 didn't open");
            std::thread::sleep(Duration::from_millis(5));
        }
        // The devices, after the displays.
        let menu = |cx: &mut VisualTestContext| {
            let outputs = TestAppContext::update(cx, Outputs::now);
            listed(&display_menu(&outputs))
                .into_iter()
                .skip(outputs.displays.len())
                .collect::<Vec<_>>()
        };
        assert_eq!(menu(&mut cx), [("Living Room".into(), false)]);

        window
            .update(&mut cx, |workspace, _, cx| {
                workspace.with_active(cx, |view, cx| view.cast_to(Some(tv.clone()), cx))
            })
            .unwrap();
        assert_eq!(menu(&mut cx), [("Living Room".into(), true)]);
    }

    /// The window with `file` open in it, its media known, and casting
    /// that has found a TV and a speaker.
    fn open_with_devices(cx: &mut TestAppContext, file: &str) -> VisualTestContext {
        let (window, mut cx) = testing::start(cx);
        let fake = FakeTarget::default();
        fake.add("Living Room", Shows::Video);
        fake.add("Kitchen", Shows::Sound);
        cx.update(|_, cx| cx.set_global(Casting::local(vec![Arc::new(fake.clone())])));
        window
            .update(&mut cx, |workspace, _, cx| {
                workspace.open_paths(vec![testing::fixture(file)], cx)
            })
            .unwrap();
        let start = Instant::now();
        let known = |cx: &mut VisualTestContext| {
            let mut known = false;
            window
                .update(cx, |workspace, _, cx| {
                    workspace.with_active(cx, |view, _| known = view.player().info().is_some())
                })
                .unwrap();
            known
        };
        while !known(&mut cx) {
            assert!(start.elapsed() < OPEN_WAIT, "{file} didn't open");
            std::thread::sleep(Duration::from_millis(5));
            cx.run_until_parked();
        }
        cx
    }

    #[gpui::test]
    fn lists_the_speakers_for_a_tile_with_sound(cx: &mut TestAppContext) {
        let cx = open_with_devices(cx, "clip.mp4");
        let outputs = TestAppContext::update(&cx, Outputs::now);
        let names = |devices: &[Device]| {
            devices
                .iter()
                .map(|device| device.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&outputs.speakers), ["Kitchen"]);
        assert_eq!(names(&outputs.devices), ["Living Room"]);
    }

    #[gpui::test]
    fn lists_no_speakers_for_a_tile_without_sound(cx: &mut TestAppContext) {
        let cx = open_with_devices(cx, "clip-ip.mkv");
        let outputs = TestAppContext::update(&cx, Outputs::now);
        assert!(outputs.speakers.is_empty(), "{:?}", outputs.speakers);
        assert_eq!(outputs.devices.len(), 1, "the TV still");
    }
}
