//! The menu bar. Its items are commands (`crate::api`), titled as they
//! are, checked when what they change is on, as the commands' states say,
//! and enabled when they can run, as the API says when the menu opens.
//! Items show their commands' keys, so the keymap sets the menus again
//! whenever it changes; so does each command run, the audio engine as
//! outputs come and go, Settings as they change, the displays as they come
//! and go or the window in front moves to another, the devices to cast to
//! as they come and go or the tile in front casts to one, and the tile in
//! front's chapters as another is in front.

use std::collections::BTreeMap;

use gpui::{App, Global, Menu, MenuItem, SystemMenuType};
use serde_json::{Value, json};

use super::speakers::output_kind;
use super::{AudioEngine, player_windows};
use crate::api::commands::{cast, output, playback, video, window};
use crate::api::{self, Run, Target, front_window};
use crate::cast::{Casting, Device, Shows};
use crate::media::output::DeviceInfo;
use crate::settings::{AudioDevice, Settings};
use crate::ui::displays::{self, DisplayInfo};
use crate::ui::framing::{ASPECT_RATIOS, CROPS, Ratio};
use crate::ui::theme::motion::DISPLAYS_POLL;

/// Set the menu bar as things are now.
pub(super) fn set_menus(cx: &mut App) {
    let now = MenuState::now(cx);
    show(now, cx);
}

/// Set the menu bar from `state`, which is kept as what it shows.
fn show(state: MenuState, cx: &mut App) {
    cx.set_global(state);
    let menus = menus(cx);
    cx.set_menus(menus);
}

/// The menu bar, as it shows things (as they are, before it's set).
fn menus(cx: &mut App) -> Vec<Menu> {
    let state = cx
        .try_global::<MenuState>()
        .cloned()
        .unwrap_or_else(|| MenuState::now(cx));
    let devices = AudioEngine::devices(cx);
    let picked: Option<AudioDevice> = state.read("output.set_device");
    let own: Option<AudioDevice> = state.read("output.set_window_device");
    let outputs = output_menu(devices, picked.as_ref(), &state);
    let window_outputs = window_output_menu(devices, own.as_ref());
    vec![
        plays_menu(&state),
        file_menu(&state),
        playback_menu(&state),
        video_menu(&state),
        audio_menu(outputs, window_outputs, &state),
        subtitles_menu(&state),
        window_menu(&state),
    ]
}

/// The item for `command`: its title, checked when it's on.
fn item(command: &'static str, state: &MenuState) -> MenuItem {
    let title = api::find(command).map_or(command, |spec| spec.title);
    MenuItem::action(title, Run::command(command)).checked(state.is_on(command))
}

/// Items for `commands`, a separator for each empty one.
fn items(commands: &[&'static str], state: &MenuState) -> Vec<MenuItem> {
    commands
        .iter()
        .map(|&command| match command {
            "" => MenuItem::separator(),
            command => item(command, state),
        })
        .collect()
}

/// The app's own menu.
fn plays_menu(state: &MenuState) -> Menu {
    let mut items = items(&["app.about", "", "app.settings", ""], state);
    items.extend([
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        item("app.quit", state),
    ]);
    Menu {
        name: "Plays".into(),
        items,
    }
}

/// Opening and closing files and windows.
fn file_menu(state: &MenuState) -> Menu {
    Menu {
        name: "File".into(),
        items: items(
            &[
                "window.open_files",
                "window.open_url",
                "",
                "tile.close",
                "tile.close_all",
                "window.close",
            ],
            state,
        ),
    }
}

/// Playing the tile in front, its chapters, and every tile.
fn playback_menu(state: &MenuState) -> Menu {
    let mut menu = items(
        &[
            "playback.play_pause",
            "playback.restart",
            "playback.toggle_loop",
            "",
            "playback.next_frame",
            "playback.previous_frame",
            "playback.next_chapter",
            "playback.previous_chapter",
            "playback.seek_forward",
            "playback.seek_backward",
            "playback.previous_file",
            "playback.next_file",
        ],
        state,
    );
    menu.extend(
        chapter_menu(&state.chapters, state.state("playback.go_to_chapter")).map(MenuItem::submenu),
    );
    menu.extend(items(
        &[
            "",
            "playback.play_pause_all",
            "playback.play_all",
            "playback.pause_all",
            "playback.restart_all",
            "playback.seek_forward_all",
            "playback.seek_backward_all",
            "playback.next_frame_all",
            "playback.previous_frame_all",
            "playback.sync_all",
        ],
        state,
    ));
    Menu {
        name: "Playback".into(),
        items: menu,
    }
}

/// The window and its display, and the tile in front's picture.
fn video_menu(state: &MenuState) -> Menu {
    let mut menu = items(
        &[
            "window.toggle_full_screen",
            "window.match_video_size",
            "tile.toggle_picture_in_picture",
            "window.toggle_float_on_top",
            "window.toggle_float_on_top_while_playing",
        ],
        state,
    );
    menu.push(MenuItem::submenu(display_menu(state)));
    menu.extend(items(
        &[
            "",
            "video.next_track",
            "",
            "video.rotate_right",
            "video.rotate_left",
            "video.flip_horizontally",
            "video.flip_vertically",
            "",
        ],
        state,
    ));
    menu.extend([
        MenuItem::submenu(aspect_menu(state.state("video.set_aspect_ratio"), state)),
        MenuItem::submenu(crop_menu(state.state("video.set_crop"), state)),
    ]);
    menu.extend(items(
        &[
            "",
            "video.reset_picture",
            "",
            "video.take_screenshot",
            "",
            "tile.toggle_console",
        ],
        state,
    ));
    Menu {
        name: "Video".into(),
        items: menu,
    }
}

/// The sound of the tile in front and every tile, and where it plays.
fn audio_menu(outputs: Menu, window_outputs: Menu, state: &MenuState) -> Menu {
    let mut menu = items(
        &[
            "audio.volume_up",
            "audio.volume_down",
            "audio.toggle_mute",
            "",
            "audio.next_track",
            "",
            "audio.earlier",
            "audio.later",
            "audio.reset_delay",
            "",
            "audio.volume_up_all",
            "audio.volume_down_all",
            "audio.toggle_mute_all",
            "",
        ],
        state,
    );
    menu.extend([
        MenuItem::submenu(outputs),
        MenuItem::submenu(window_outputs),
    ]);
    Menu {
        name: "Audio".into(),
        items: menu,
    }
}

/// The subtitles of the tile in front.
fn subtitles_menu(state: &MenuState) -> Menu {
    Menu {
        name: "Subtitles".into(),
        items: items(
            &[
                "subtitles.toggle",
                "subtitles.next_track",
                "",
                "subtitles.bigger",
                "subtitles.smaller",
                "subtitles.up",
                "subtitles.down",
                "",
                "subtitles.earlier",
                "subtitles.later",
                "subtitles.reset_delay",
            ],
            state,
        ),
    }
}

/// Opening, naming and closing windows, going from tile to tile in the
/// window in front and moving them about, and going from window to window
/// and numbering them. Named so, macOS adds its own window items to it.
fn window_menu(state: &MenuState) -> Menu {
    let mut items = items(
        &[
            "window.new",
            "window.rename",
            "window.close_others",
            "",
            "tile.focus.left",
            "tile.focus.right",
            "tile.focus.up",
            "tile.focus.down",
            "tile.focus.previous",
            "tile.focus.next",
            "",
            "tile.move.left",
            "tile.move.right",
            "tile.move.up",
            "tile.move.down",
            "",
            // ⌘1 to ⌘9 go to a window by its number; macOS lists the
            // windows below.
            "window.next",
            "window.previous",
            "window.back",
            "window.recent",
            "",
            "window.move.backward",
            "window.move.forward",
        ],
        state,
    );
    items.extend(number_menu(state).map(MenuItem::submenu));
    Menu {
        name: "Window".into(),
        items,
    }
}

/// A number for each player window, the one in front's checked, to give
/// it that number instead; none for a window alone.
fn number_menu(state: &MenuState) -> Option<Menu> {
    let front = state.state("window.move").as_u64();
    (state.windows > 1).then(|| Menu {
        name: title("window.move").into(),
        items: (1..=state.windows)
            .map(|number| {
                let checked = front == u64::try_from(number).ok();
                MenuItem::action(number.to_string(), window::move_to(number)).checked(checked)
            })
            .collect(),
    })
}

/// Check the menus again when Settings change: floating on top, and the
/// output picked.
pub(super) fn watch_settings(cx: &mut App) {
    cx.observe_global::<Settings>(refresh).detach();
}

/// What the menus show that changes without them hearing of it: besides
/// this computer's outputs, Video → Output Device and Audio → Output Device
/// show the displays, the one the window in front is on, the devices that
/// show video, the speakers, and the one the tile in front casts to;
/// Playback → Chapters lists the tile in front's chapters; and what each
/// command changes, for the window and tile in front, checks its items.
#[derive(Clone, Default, PartialEq, Debug)]
struct MenuState {
    displays: Vec<DisplayInfo>,
    front: Option<u32>,
    devices: Vec<Device>,
    /// Those for the tile in front: none when it has no sound.
    speakers: Vec<Device>,
    casting: Option<Device>,
    /// The tile in front's chapters, by name.
    chapters: Vec<String>,
    /// How many player windows are open, each a number to move one to.
    windows: usize,
    /// The state of each command that has one, as it is for the window
    /// and tile in front.
    states: BTreeMap<&'static str, Value>,
}

impl Global for MenuState {}

impl MenuState {
    fn now(cx: &mut App) -> Self {
        let window = front_window(cx);
        let (front, casting, has_audio, chapters) = window
            .and_then(|handle| {
                handle
                    .update(cx, |workspace, window, cx| {
                        (
                            displays::of(window, cx),
                            workspace.active_casting(cx),
                            workspace.active_has_audio(cx),
                            workspace.active_chapters(cx).0,
                        )
                    })
                    .ok()
            })
            .unwrap_or((None, None, true, Vec::new()));
        let (devices, speakers) = Casting::devices_for(cx, has_audio)
            .into_iter()
            .partition(|device| device.shows == Shows::Video);
        let states = api::all()
            .filter(|spec| spec.state.is_some())
            .map(|spec| {
                let state = api::state(spec.id, &Target::default(), cx);
                (spec.id, state.unwrap_or(Value::Null))
            })
            .collect();
        Self {
            // As last looked at: asking the system is slow on X11, and this
            // is made every second and as the mouse goes between tiles.
            displays: displays::connected(cx),
            front,
            devices,
            speakers,
            casting,
            chapters,
            windows: player_windows(cx).len(),
            states,
        }
    }

    /// The state of `command`, null for none.
    fn state(&self, command: &str) -> &Value {
        self.states.get(command).unwrap_or(&Value::Null)
    }

    /// Whether `command` is on: its state is true.
    fn is_on(&self, command: &str) -> bool {
        *self.state(command) == Value::Bool(true)
    }

    /// The state of `command`, as `T`, if it is one.
    fn read<T: serde::de::DeserializeOwned + Default>(&self, command: &str) -> T {
        serde_json::from_value(self.state(command).clone()).unwrap_or_default()
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

/// Set the menus again once the current event is handled, if what they
/// show has changed: from a window or tile, which can't be read while
/// they're being changed.
pub fn refresh(cx: &mut App) {
    cx.defer(refresh_outputs);
}

fn refresh_outputs(cx: &mut App) {
    let now = MenuState::now(cx);
    if cx.try_global::<MenuState>() != Some(&now) {
        show(now, cx);
    }
}

/// The tile in front's chapters, the one it's in, `chapter` (from 1),
/// checked; none for a file without.
fn chapter_menu(chapters: &[String], chapter: &Value) -> Option<Menu> {
    (!chapters.is_empty()).then(|| Menu {
        name: "Chapters".into(),
        items: chapters
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let number = index + 1;
                MenuItem::action(name.clone(), playback::go_to_chapter(number))
                    .checked(*chapter == json!(number))
            })
            .collect(),
    })
}

/// The aspect ratios, the tile in front's, `aspect` (by name, null as the
/// file is), checked.
fn aspect_menu(aspect: &Value, state: &MenuState) -> Menu {
    Menu {
        name: title("video.set_aspect_ratio").into(),
        items: [(None, "As the File Is")]
            .into_iter()
            .chain(ASPECT_RATIOS.map(|r| (Some(r), r.name)))
            .map(|(ratio, name)| {
                MenuItem::action(name, video::set_aspect_ratio(ratio))
                    .checked(*aspect == named(ratio))
            })
            .chain(items(&["", "video.next_aspect_ratio"], state))
            .collect(),
    }
}

/// The crops, the tile in front's, `crop` (by name, null for none),
/// checked.
fn crop_menu(crop: &Value, state: &MenuState) -> Menu {
    Menu {
        name: title("video.set_crop").into(),
        items: [(None, "None")]
            .into_iter()
            .chain(CROPS.map(|r| (Some(r), r.name)))
            .map(|(ratio, name)| {
                MenuItem::action(name, video::set_crop(ratio)).checked(*crop == named(ratio))
            })
            .chain(items(&["", "video.next_crop"], state))
            .collect(),
    }
}

/// A ratio as its command's state says it: by name, or null.
fn named(ratio: Option<Ratio>) -> Value {
    json!(ratio.map(|ratio| ratio.name))
}

/// The title of `command`.
fn title(command: &'static str) -> &'static str {
    api::find(command).map_or(command, |spec| spec.title)
}

/// Every display, the one the window in front is on checked, then the
/// devices that show video, the one the tile in front casts to checked
/// instead (a speaker it casts to leaves its picture here). Picking a
/// display shows the window there, full screen; picking a device casts the
/// tile in front to it.
fn display_menu(outputs: &MenuState) -> Menu {
    let casting = outputs
        .casting
        .as_ref()
        .filter(|device| device.shows == Shows::Video);
    let mut items: Vec<MenuItem> = outputs
        .displays
        .iter()
        .map(|display| {
            MenuItem::action(display.name.clone(), window::show_on_display(display.id))
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
    let names: Vec<&str> = (outputs.displays.iter().map(|display| display.name.as_str()))
        .chain(devices.iter().map(|device| device.name.as_str()))
        .collect();
    let titles: Vec<String> = devices
        .iter()
        .map(|device| told_apart(&device.name, Some(device.kind), &names))
        .collect();
    items.extend(devices.into_iter().zip(titles).map(|(device, title)| {
        let checked = casting.is_some_and(|casting| casting.is(&device));
        MenuItem::action(title, cast::to(Some(&device))).checked(checked)
    }));
    Menu {
        name: "Output Device".into(),
        items,
    }
}

/// What an item for a device called `name` is titled, among items called
/// `names` (its own too): its name, with the protocol it's reached by,
/// `kind`, after it where another item has the same name. So a device
/// found by two protocols, as a speaker with AirPlay and DLNA, shows as
/// two; displays and this computer's outputs, reached by none, keep their
/// names.
fn told_apart(name: &str, kind: Option<&str>, names: &[&str]) -> String {
    let shared = names.iter().filter(|&&other| other == name).count() > 1;
    match kind {
        Some(kind) if shared => format!("{name} ({kind})"),
        _ => name.to_owned(),
    }
}

/// The system's default and every output connected, with the one picked
/// checked; one picked that isn't connected is listed too, as it is still
/// what the sound waits for. Then the speakers, and the device the tile in
/// front casts to, checked instead: picking one casts the tile in front to
/// it.
fn output_menu(devices: &[DeviceInfo], picked: Option<&AudioDevice>, outputs: &MenuState) -> Menu {
    let casting = outputs.casting.as_ref();
    let here = casting.is_none();
    let mut speakers = outputs.speakers.clone();
    // What it casts to plays its sound, and stays listed while it isn't
    // found.
    if let Some(casting) = casting
        && !speakers.iter().any(|device| device.is(casting))
    {
        speakers.push(casting.clone());
    }
    let names: Vec<&str> = (devices.iter().map(|device| device.name.as_str()))
        .chain(speakers.iter().map(|device| device.name.as_str()))
        .collect();
    let speaker_titles: Vec<String> = speakers
        .iter()
        .map(|device| told_apart(&device.name, Some(device.kind), &names))
        .collect();
    let mut items = vec![
        MenuItem::action("System Default", output::set_device(None))
            .checked(here && picked.is_none()),
        MenuItem::separator(),
    ];
    items.extend(devices.iter().map(|device| {
        let checked = here && picked.is_some_and(|picked| picked.id == device.id);
        let title = told_apart(&device.name, output_kind(&device.id), &names);
        let device = AudioDevice {
            id: device.id.clone(),
            name: device.name.clone(),
        };
        MenuItem::action(title, output::set_device(Some(&device))).checked(checked)
    }));
    if let Some(picked) = picked
        && !devices.iter().any(|device| device.id == picked.id)
    {
        items.push(
            MenuItem::action(
                format!("{} (Not Connected)", picked.name),
                output::set_device(Some(picked)),
            )
            .checked(here),
        );
    }
    if !speakers.is_empty() {
        items.push(MenuItem::separator());
    }
    items.extend(
        speakers
            .into_iter()
            .zip(speaker_titles)
            .map(|(device, title)| {
                let checked = casting.is_some_and(|casting| casting.is(&device));
                MenuItem::action(title, cast::to(Some(&device))).checked(checked)
            }),
    );
    Menu {
        name: "Output Device".into(),
        items,
    }
}

/// The app's output, checked unless the window in front plays through a
/// device of its own, then every output connected, its own checked; its
/// own is listed too while it isn't connected, as its sound waits for it.
fn window_output_menu(devices: &[DeviceInfo], own: Option<&AudioDevice>) -> Menu {
    let mut items = vec![
        MenuItem::action("Same as the App", output::set_window_device(None)).checked(own.is_none()),
        MenuItem::separator(),
    ];
    let names: Vec<&str> = devices.iter().map(|device| device.name.as_str()).collect();
    items.extend(devices.iter().map(|device| {
        let checked = own.is_some_and(|own| own.id == device.id);
        let title = told_apart(&device.name, output_kind(&device.id), &names);
        let device = AudioDevice {
            id: device.id.clone(),
            name: device.name.clone(),
        };
        MenuItem::action(title, output::set_window_device(Some(&device))).checked(checked)
    }));
    if let Some(own) = own
        && !devices.iter().any(|device| device.id == own.id)
    {
        items.push(
            MenuItem::action(
                format!("{} (Not Connected)", own.name),
                output::set_window_device(Some(own)),
            )
            .checked(true),
        );
    }
    Menu {
        name: "Window Output Device".into(),
        items,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use gpui::{Bounds, TestAppContext, VisualTestContext};

    use super::*;
    use crate::cast::fake::FakeTarget;
    use crate::ui::testing::{self, OPEN_WAIT};
    use crate::ui::theme::layout::WINDOW_SIZE;
    use crate::ui::workspace::Workspace;

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

    /// The items checked in `menu`.
    fn checked(menu: &Menu) -> Vec<String> {
        listed(menu)
            .into_iter()
            .filter(|(_, checked)| *checked)
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn lists_the_chapters_checking_the_one_the_tile_is_in() {
        assert!(
            chapter_menu(&[], &Value::Null).is_none(),
            "none for a file without"
        );
        let names = ["Opening".to_owned(), "Middle".to_owned()];
        let menu = chapter_menu(&names, &json!(2)).unwrap();
        assert_eq!(
            listed(&menu),
            [("Opening".into(), false), ("Middle".into(), true)]
        );
    }

    #[gpui::test]
    fn lists_the_displays_as_last_looked_at(cx: &mut TestAppContext) {
        let (_, cx) = testing::start(cx);
        // Looked at every second, not each time the menus are made: asking
        // is slow on X11.
        TestAppContext::update(&cx, |cx| {
            let mut seen = displays::list(cx);
            seen.push(display(7, "Projector"));
            displays::remember(seen, cx);
        });
        let state = TestAppContext::update(&cx, MenuState::now);
        assert_eq!(
            state.displays.last().map(|display| display.name.as_str()),
            Some("Projector"),
            "{:?}",
            state.displays
        );
    }

    #[gpui::test]
    fn the_chapters_menu_follows_the_tile_in_front(cx: &mut TestAppContext) {
        let (window, mut cx) = testing::start(cx);
        window
            .update(&mut cx, |workspace, _, cx| {
                workspace.open_paths(vec![testing::fixture("clip-chapters.mkv")], cx)
            })
            .unwrap();
        let known = testing::wait_until(&mut cx, |cx| {
            let state = TestAppContext::update(cx, MenuState::now);
            !state.chapters.is_empty()
        });
        assert!(known, "its chapters are known");
        let state = TestAppContext::update(&cx, MenuState::now);
        assert_eq!(state.chapters, ["Opening", "Middle", "End"]);
        assert_eq!(state.state("playback.go_to_chapter"), &json!(1));

        cx.dispatch_action(playback::go_to_chapter(3));
        cx.run_until_parked();
        let shown = cx.update(|_, cx| {
            let state = cx.try_global::<MenuState>().cloned().unwrap_or_default();
            state.state("playback.go_to_chapter").clone()
        });
        assert_eq!(shown, json!(3), "at once");
    }

    #[test]
    fn the_video_menu_sizes_the_window_for_its_videos() {
        let menu = video_menu(&MenuState::default());
        let matches = menu.items.iter().any(|item| match item {
            MenuItem::Action { name, action, .. } => {
                name == "Match Video Size"
                    && action.partial_eq(&Run::command("window.match_video_size"))
            }
            _ => false,
        });
        assert!(matches, "no Match Video Size in the Video menu");
    }

    #[test]
    fn the_window_menu_goes_to_the_next_window_or_the_one_before() {
        let menu = window_menu(&MenuState::default());
        let has = |title: &str, command: &str| {
            menu.items.iter().any(|item| match item {
                MenuItem::Action { name, action, .. } => {
                    name == title && action.partial_eq(&Run::command(command))
                }
                _ => false,
            })
        };
        assert!(has("Next Window", "window.next"), "no Next Window");
        assert!(
            has("Previous Window", "window.previous"),
            "no Previous Window"
        );
    }

    #[test]
    fn the_window_menu_opens_names_closes_goes_back_to_and_moves_windows() {
        let state = MenuState {
            windows: 3,
            states: BTreeMap::from([("window.move", json!(2))]),
            ..MenuState::default()
        };
        let menu = window_menu(&state);
        let has = |title: &str, command: &str| {
            menu.items.iter().any(|item| match item {
                MenuItem::Action { name, action, .. } => {
                    name == title && action.partial_eq(&Run::command(command))
                }
                _ => false,
            })
        };
        for (title, command) in [
            ("New Window", "window.new"),
            ("Rename Window…", "window.rename"),
            ("Close Other Windows", "window.close_others"),
            ("Go Back One Window", "window.back"),
            ("Go to Recent Window", "window.recent"),
            ("Move Window Backward", "window.move.backward"),
            ("Move Window Forward", "window.move.forward"),
        ] {
            assert!(has(title, command), "no {title}");
        }

        // A number for each window, the one in front's checked.
        let numbers = menu.items.iter().find_map(|item| match item {
            MenuItem::Submenu(menu) if menu.name == "Move Window To" => Some(menu),
            _ => None,
        });
        let numbers = numbers.expect("no Move Window To");
        let items: Vec<(String, bool)> = numbers
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Action {
                    name,
                    action,
                    checked,
                    ..
                } => {
                    let number: usize = name.parse().expect("a number");
                    assert!(action.partial_eq(&window::move_to(number)), "{name}");
                    (name.to_string(), *checked)
                }
                _ => panic!("a number"),
            })
            .collect();
        let expected = [("1", false), ("2", true), ("3", false)].map(|(n, c)| (n.to_owned(), c));
        assert_eq!(items, expected);

        let alone = window_menu(&MenuState {
            windows: 1,
            ..MenuState::default()
        });
        let numbered = alone
            .items
            .iter()
            .any(|item| matches!(item, MenuItem::Submenu(menu) if menu.name == "Move Window To"));
        assert!(!numbered, "nowhere to move one alone");
    }

    #[test]
    fn checks_the_aspect_ratio_and_crop_of_the_tile_in_front() {
        let state = MenuState::default();
        assert_eq!(
            checked(&aspect_menu(&Value::Null, &state)),
            ["As the File Is"]
        );
        let ratio = ASPECT_RATIOS[1];
        assert_eq!(checked(&aspect_menu(&json!(ratio.name), &state)), ["4:3"]);
        assert_eq!(checked(&crop_menu(&Value::Null, &state)), ["None"]);
        let crop = CROPS[CROPS.len() - 1];
        assert_eq!(checked(&crop_menu(&json!(crop.name), &state)), [crop.name]);
    }

    #[gpui::test]
    fn the_menus_number_the_windows_as_they_open_and_close(cx: &mut TestAppContext) {
        let (_, mut cx) = testing::start(cx);
        let shown = |cx: &mut VisualTestContext| {
            cx.run_until_parked();
            cx.update(|_, cx| {
                let state = cx.try_global::<MenuState>().cloned().unwrap_or_default();
                (state.windows, state.state("window.move").clone())
            })
        };
        let open = |cx: &mut VisualTestContext| {
            TestAppContext::update(cx, |cx| Workspace::open_window(WINDOW_SIZE, cx)).unwrap()
        };

        open(&mut cx);
        let third = open(&mut cx);
        assert_eq!(shown(&mut cx), (3, json!(1)), "the first in front");
        third
            .update(&mut cx, |_, window, _| window.remove_window())
            .unwrap();
        assert_eq!(shown(&mut cx).0, 2, "one closed");
    }

    #[gpui::test]
    fn the_menus_follow_the_picture_of_the_tile_in_front_at_once(cx: &mut TestAppContext) {
        let (window, mut cx) = testing::start(cx);
        window
            .update(&mut cx, |workspace, _, cx| {
                let files = ["clip.mp4", "clip-ip.mkv"].map(testing::fixture);
                workspace.open_paths(files.into(), cx)
            })
            .unwrap();
        let start = Instant::now();
        while !testing::shows(&mut cx, "tile-1") {
            assert!(start.elapsed() < OPEN_WAIT, "the files didn't open");
            std::thread::sleep(Duration::from_millis(5));
        }
        // Each tile's media known, the mouse over it.
        for tile in ["tile-1", "tile-0"] {
            testing::hover(&mut cx, tile);
            let known = testing::wait_until(&mut cx, |cx| {
                let mut known = false;
                window
                    .update(cx, |workspace, _, cx| {
                        workspace.with_active(cx, |view, _| known = view.player().is_known())
                    })
                    .unwrap();
                known
            });
            assert!(known, "{tile}'s media is known");
        }
        let shown = |cx: &mut VisualTestContext| {
            let state = cx.update(|_, cx| cx.try_global::<MenuState>().cloned());
            state.map(|state| {
                let read = |command| state.state(command).clone();
                (read("video.set_aspect_ratio"), read("video.set_crop"))
            })
        };
        let crop = CROPS[0];
        let ratio = json!(ASPECT_RATIOS[0].name);
        let none = Value::Null;

        cx.dispatch_action(video::set_crop(Some(crop)));
        cx.run_until_parked();
        assert_eq!(shown(&mut cx), Some((none.clone(), json!(crop.name))));

        testing::hover(&mut cx, "tile-1");
        assert_eq!(
            shown(&mut cx),
            Some((none.clone(), none.clone())),
            "the other tile's"
        );
        cx.dispatch_action(Run::command("video.next_aspect_ratio"));
        cx.run_until_parked();
        assert_eq!(shown(&mut cx), Some((ratio.clone(), none.clone())));

        testing::hover(&mut cx, "tile-0");
        assert_eq!(shown(&mut cx), Some((none.clone(), json!(crop.name))));

        // Gone to from the keyboard: the other tile's again.
        cx.dispatch_action(Run::command("tile.focus.next"));
        cx.run_until_parked();
        assert_eq!(shown(&mut cx), Some((ratio, none)));
    }

    #[test]
    fn checks_the_output_picked() {
        let devices = [device("a", "Speakers"), device("b", "Headphones")];
        assert_eq!(
            listed(&output_menu(&devices, None, &MenuState::default())),
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
            listed(&output_menu(&devices, Some(&picked), &MenuState::default())),
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
            bounds: Bounds::default(),
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
        let mut outputs = MenuState {
            speakers: vec![speaker("k", "Kitchen"), speaker("p", "Phantom")],
            ..MenuState::default()
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
        let outputs = MenuState {
            displays: vec![display(1, "Built-in Display")],
            front: Some(1),
            devices: vec![tv("t", "Living Room")],
            speakers: vec![speaker("k", "Kitchen")],
            casting: Some(speaker("k", "Kitchen")),
            ..MenuState::default()
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
        let outputs = MenuState {
            displays: vec![display(1, "Built-in Display"), display(7, "Beamer")],
            front: Some(7),
            ..MenuState::default()
        };
        assert_eq!(
            listed(&display_menu(&outputs)),
            [("Built-in Display".into(), false), ("Beamer".into(), true),]
        );
    }

    #[test]
    fn checks_the_device_the_tile_in_front_casts_to_instead() {
        let mut outputs = MenuState {
            displays: vec![display(1, "Built-in Display")],
            front: Some(1),
            devices: vec![tv("a", "Bedroom"), tv("b", "Living Room")],
            ..MenuState::default()
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
    fn a_device_found_by_two_protocols_shows_as_two() {
        let dlna = |id: &str, name: &str| Device {
            kind: "DLNA",
            ..speaker(id, name)
        };
        // The Phantom as an AirPlay speaker, an output, and with DLNA, a
        // speaker cast to.
        let devices = [device("desk", "Desk"), device("airplay:ph", "Phantom")];
        let outputs = MenuState {
            speakers: vec![dlna("uuid-1", "Phantom"), speaker("k", "Kitchen")],
            ..MenuState::default()
        };
        assert_eq!(
            listed(&output_menu(&devices, None, &outputs)),
            [
                ("System Default".into(), true),
                ("Desk".into(), false),
                ("Phantom (AirPlay)".into(), false),
                ("Phantom (DLNA)".into(), false),
                ("Kitchen".into(), false),
            ]
        );
        assert_eq!(
            listed(&window_output_menu(
                &[device("hdmi", "Beamer"), device("airplay:tv", "Beamer")],
                None
            )),
            [
                ("Same as the App".into(), true),
                ("Beamer".into(), false),
                ("Beamer (AirPlay)".into(), false),
            ],
            "this computer's own output keeps its name"
        );
        // A TV with two, beside a display of that name.
        let outputs = MenuState {
            displays: vec![display(1, "Living Room")],
            devices: vec![tv("a", "Living Room"), dlna("b", "Living Room")],
            ..MenuState::default()
        };
        assert_eq!(
            listed(&display_menu(&outputs)),
            [
                ("Living Room".into(), false),
                ("Living Room (Fake)".into(), false),
                ("Living Room (DLNA)".into(), false),
            ]
        );
        let outputs = MenuState {
            devices: vec![tv("a", "Living Room")],
            ..outputs
        };
        assert_eq!(
            listed(&display_menu(&outputs)),
            [
                ("Living Room".into(), false),
                ("Living Room (Fake)".into(), false),
            ],
            "told apart from the display too"
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
                &MenuState::default()
            )),
            [
                ("System Default".into(), false),
                ("Speakers".into(), false),
                ("TV (Not Connected)".into(), true),
            ]
        );
    }

    #[test]
    fn checks_the_output_the_window_in_front_plays_through() {
        let devices = [device("a", "Speakers"), device("b", "TV")];
        assert_eq!(
            listed(&window_output_menu(&devices, None)),
            [
                ("Same as the App".into(), true),
                ("Speakers".into(), false),
                ("TV".into(), false),
            ]
        );
        let own = AudioDevice {
            id: "b".into(),
            name: "TV".into(),
        };
        assert_eq!(
            listed(&window_output_menu(&devices, Some(&own))),
            [
                ("Same as the App".into(), false),
                ("Speakers".into(), false),
                ("TV".into(), true),
            ]
        );
        assert_eq!(
            listed(&window_output_menu(&devices[..1], Some(&own))),
            [
                ("Same as the App".into(), false),
                ("Speakers".into(), false),
                ("TV (Not Connected)".into(), true),
            ],
            "listed while it is away"
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
            let outputs = TestAppContext::update(cx, MenuState::now);
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
                    workspace.with_active(cx, |view, _| known = view.player().is_known())
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
        let outputs = TestAppContext::update(&cx, MenuState::now);
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
        let outputs = TestAppContext::update(&cx, MenuState::now);
        assert!(outputs.speakers.is_empty(), "{:?}", outputs.speakers);
        assert_eq!(outputs.devices.len(), 1, "the TV still");
    }

    /// Every item in `menus`, submenus' too, with its title.
    fn every_item(menus: &[Menu]) -> Vec<(String, Box<dyn gpui::Action>, bool)> {
        fn walk(items: &[MenuItem], found: &mut Vec<(String, Box<dyn gpui::Action>, bool)>) {
            for item in items {
                match item {
                    MenuItem::Action {
                        name,
                        action,
                        checked,
                        ..
                    } => found.push((name.to_string(), action.boxed_clone(), *checked)),
                    MenuItem::Submenu(menu) => walk(&menu.items, found),
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        for menu in menus {
            walk(&menu.items, &mut found);
        }
        found
    }

    /// The command an item runs.
    fn invocation(action: &dyn gpui::Action) -> api::Invocation {
        let run = action.as_any().downcast_ref::<Run>();
        run.expect("items run commands").0.clone()
    }

    #[gpui::test]
    fn every_item_runs_a_command_titled_as_it_is(cx: &mut TestAppContext) {
        let (window, mut cx) = testing::start(cx);
        window
            .update(&mut cx, |workspace, _, cx| {
                workspace.open_paths(vec![testing::fixture("clip-chapters.mkv")], cx)
            })
            .unwrap();
        let known = testing::wait_until(&mut cx, |cx| {
            let state = TestAppContext::update(cx, MenuState::now);
            !state.chapters.is_empty()
        });
        assert!(known, "its chapters are known");
        let items = TestAppContext::update(&cx, |cx| {
            let state = MenuState::now(cx);
            cx.set_global(state);
            every_item(&menus(cx))
        });

        let mut commands = std::collections::HashSet::new();
        for (name, action, _) in &items {
            let invocation = invocation(&**action);
            let spec = api::find(&invocation.command)
                .unwrap_or_else(|| panic!("{name}: no command {}", invocation.command));
            api::check_args(spec, &invocation.args).unwrap_or_else(|err| panic!("{name}: {err}"));
            if spec.args.is_empty() {
                assert_eq!(name, spec.title, "{}", spec.id);
            }
            commands.insert(spec.id);
        }
        // Each command once, but those with arguments, given each.
        let takes_args = |action: &dyn gpui::Action| {
            let command = invocation(action).command;
            !api::find(&command).unwrap().args.is_empty()
        };
        let plain = items
            .iter()
            .filter(|(_, action, _)| !takes_args(&**action))
            .count();
        let with_args: std::collections::HashSet<_> = items
            .iter()
            .filter(|(_, action, _)| takes_args(&**action))
            .map(|(_, action, _)| invocation(&**action).command)
            .collect();
        assert_eq!(plain + with_args.len(), commands.len(), "one item each");
        for command in [
            "playback.go_to_chapter",
            "video.set_aspect_ratio",
            "video.set_crop",
            "output.set_device",
            "output.set_window_device",
            "window.show_on_display",
        ] {
            assert!(commands.contains(command), "no item runs {command}");
        }
    }

    #[gpui::test]
    fn checks_what_is_on_and_enables_what_can_run(cx: &mut TestAppContext) {
        let (window, mut cx) = testing::start(cx);
        let enabled = |command: &str, cx: &mut TestAppContext| {
            TestAppContext::update(cx, |cx| cx.is_menu_action_enabled(&Run::command(command)))
        };
        // As the menu bar was last set.
        let checked = |title: &str, cx: &mut TestAppContext| {
            let items = TestAppContext::update(cx, |cx| {
                assert!(cx.try_global::<MenuState>().is_some(), "the menus are set");
                every_item(&menus(cx))
            });
            items
                .into_iter()
                .find(|(name, _, _)| name == title)
                .is_some_and(|(_, _, checked)| checked)
        };
        // An empty window: nothing to play.
        assert!(!enabled("playback.play_pause", &mut cx));
        assert!(!enabled("tile.close", &mut cx));
        assert!(enabled("window.open_files", &mut cx));
        assert!(enabled("app.quit", &mut cx));

        window
            .update(&mut cx, |workspace, _, cx| {
                workspace.open_paths(vec![testing::fixture("clip.mp4")], cx)
            })
            .unwrap();
        let opened = testing::wait_until(&mut cx, |cx| enabled("playback.play_pause", cx));
        assert!(opened, "a file to play");
        assert!(enabled("tile.close", &mut cx));
        assert_eq!(
            enabled("tile.toggle_picture_in_picture", &mut cx),
            cfg!(target_os = "macos")
        );

        assert!(!checked("Loop", &mut cx));
        cx.dispatch_action(Run::command("playback.toggle_loop"));
        cx.run_until_parked();
        assert!(checked("Loop", &mut cx), "at once");
        assert!(!checked("Float on Top", &mut cx));
        cx.dispatch_action(Run::command("window.toggle_float_on_top"));
        cx.run_until_parked();
        assert!(checked("Float on Top", &mut cx));
        assert!(!checked("Float on Top While Playing", &mut cx));
    }
}
