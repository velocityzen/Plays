//! The menu bar. Its items are commands (`crate::api`), titled as they
//! are, checked when what they change is on, as the commands' states say,
//! and enabled when they can run, as the API says when the menu opens.
//! Items show their commands' keys, so the keymap sets the menus again
//! whenever it changes; so does each command run, the audio engine as
//! outputs come and go, Settings as they change, the displays as they come
//! and go or the window in front moves to another, the devices to cast to
//! as they come and go or the block in front casts to one, the block in
//! front's chapters as another is in front, and the files played lately
//! as they change.

use std::collections::BTreeMap;
use std::path::Path;

use gpui::{App, Global, Menu, MenuItem, SystemMenuType};
use serde_json::{Value, json};

use super::speakers::output_kind;
use super::{AudioEngine, player_windows};
use crate::api::commands::{block, cast, output, playback, video, window};
use crate::api::{self, Run, Target, front_window};
use crate::cast::{Casting, Device, Shows};
use crate::files;
use crate::media::output::DeviceInfo;
use crate::recents;
use crate::settings::{AudioDevice, Settings};
use crate::ui::displays::{self, DisplayInfo};
use crate::ui::framing::{ASPECT_RATIOS, CROPS, Ratio};
use crate::ui::player::speed::{SPEEDS, speed_text};
use crate::ui::theme::motion::DISPLAYS_POLL;
use crate::ui::workspace::Workspace;

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
    let own: Option<AudioDevice> = state.read("output.set_block_device");
    let outputs = output_menu(devices, picked.as_ref(), &state);
    let block_outputs = block_output_menu(devices, own.as_ref());
    vec![
        plays_menu(&state),
        file_menu(&state),
        playback_menu(&state),
        video_menu(&state),
        audio_menu(outputs, block_outputs, &state),
        subtitles_menu(&state),
        interface_menu(&state),
        window_menu(&state),
        #[cfg(debug_assertions)]
        developer_menu(&state),
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

/// The app's own menu: Install Command Line Tool where Plays can, on
/// macOS.
fn plays_menu(state: &MenuState) -> Menu {
    let mut items = items(
        &[
            "app.about",
            "app.check_for_updates",
            "",
            "app.settings.open",
        ],
        state,
    );
    let install = api::find("cli.install").filter(|spec| (spec.available)());
    items.extend(install.map(|spec| item(spec.id, state)));
    items.extend([
        MenuItem::separator(),
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        item("app.quit", state),
    ]);
    Menu {
        name: "Plays".into(),
        items,
    }
}

/// Tools for working on Plays itself, in debug builds only.
#[cfg(debug_assertions)]
fn developer_menu(state: &MenuState) -> Menu {
    Menu {
        name: "Developer".into(),
        items: items(&["app.show_components"], state),
    }
}

/// Opening and closing files and windows, and the files played lately.
fn file_menu(state: &MenuState) -> Menu {
    let mut items = items(&["window.open_files", "window.open_url"], state);
    items.extend(recent_menu(state).map(MenuItem::submenu));
    items.extend(self::items(
        &[
            "",
            "block.close",
            "block.close_others",
            "block.close_all",
            "window.close",
            "",
            "block.copy_path",
            "block.copy_time",
        ],
        state,
    ));
    Menu {
        name: "File".into(),
        items,
    }
}

/// The files played lately, the latest first, to open in the window in
/// front, each by its name, with its folder where two share one; and
/// Clear Recent Files. None without any.
fn recent_menu(state: &MenuState) -> Option<Menu> {
    let files = &state.recent_files;
    let named: Vec<(String, String)> = files
        .iter()
        .map(|file| {
            let path = Path::new(file);
            (
                files::name(path),
                path.parent().map(files::shown).unwrap_or_default(),
            )
        })
        .collect();
    let shared = |name: &str| named.iter().filter(|(other, _)| other == name).count() > 1;
    let mut items: Vec<MenuItem> = files
        .iter()
        .zip(&named)
        .map(|(file, (name, folder))| {
            let title = if shared(name) {
                format!("{name} — {folder}")
            } else {
                name.clone()
            };
            MenuItem::action(title, Run::with("app.open", json!({"urls": [file]})))
        })
        .collect();
    if items.is_empty() {
        return None;
    }
    items.extend([MenuItem::separator(), item("app.clear_recent_files", state)]);
    Some(Menu {
        name: "Open Recent".into(),
        items,
    })
}

/// Playing the block in front, its chapters, and every block.
fn playback_menu(state: &MenuState) -> Menu {
    let mut menu = items(
        &[
            "block.playback.play_pause",
            "block.playback.restart",
            "block.playback.loop.toggle",
        ],
        state,
    );
    menu.push(MenuItem::submenu(speed_menu(
        state.state("block.playback.set_speed"),
        state,
    )));
    menu.extend(items(
        &[
            "",
            "block.playback.next_frame",
            "block.playback.previous_frame",
            "block.playback.next_chapter",
            "block.playback.previous_chapter",
            "block.playback.seek_forward",
            "block.playback.seek_backward",
            "block.playback.previous_file",
            "block.playback.next_file",
        ],
        state,
    ));
    menu.extend(
        chapter_menu(&state.chapters, state.state("block.playback.go_to_chapter"))
            .map(MenuItem::submenu),
    );
    menu.extend(items(
        &[
            "",
            "app.playback.play_pause",
            "app.playback.play",
            "app.playback.pause",
            "app.playback.restart",
            "app.playback.seek_forward",
            "app.playback.seek_backward",
            "app.playback.next_frame",
            "app.playback.previous_frame",
            "block.playback.sync_all",
        ],
        state,
    ));
    Menu {
        name: "Playback".into(),
        items: menu,
    }
}

/// The window and its display, and the block in front's picture.
fn video_menu(state: &MenuState) -> Menu {
    let mut menu = items(
        &[
            "window.full_screen.toggle",
            "window.match_video_size",
            "block.picture_in_picture.toggle",
            "window.float_on_top.toggle",
            "window.float_on_top_while_playing.toggle",
        ],
        state,
    );
    menu.push(MenuItem::submenu(display_menu(state)));
    menu.extend(items(
        &[
            "",
            "block.video.next_track",
            "",
            "block.video.rotate_right",
            "block.video.rotate_left",
            "block.video.flip_horizontally",
            "block.video.flip_vertically",
            "",
        ],
        state,
    ));
    menu.extend([
        MenuItem::submenu(aspect_menu(
            state.state("block.video.set_aspect_ratio"),
            state,
        )),
        MenuItem::submenu(crop_menu(state.state("block.video.set_crop"), state)),
    ]);
    menu.extend(items(
        &[
            "",
            "block.video.reset_picture",
            "",
            "block.video.take_screenshot",
            "",
            "block.console.toggle",
        ],
        state,
    ));
    Menu {
        name: "Video".into(),
        items: menu,
    }
}

/// The panels over the video, all at once or each: over the block in
/// front, each block in the window in front, and every block in every
/// window.
fn interface_menu(state: &MenuState) -> Menu {
    Menu {
        name: "Interface".into(),
        items: items(
            &[
                "block.ui.toggle",
                "block.controls.toggle",
                "block.top_bar.toggle",
                "block.track_switcher.toggle",
                "block.notes.toggle",
                "",
                "window.ui.toggle",
                "window.controls.toggle",
                "window.top_bar.toggle",
                "window.track_switcher.toggle",
                "window.notes.toggle",
                "",
                "app.ui.toggle",
                "app.controls.toggle",
                "app.top_bar.toggle",
                "app.track_switcher.toggle",
                "app.notes.toggle",
            ],
            state,
        ),
    }
}

/// The sound of the block in front and every block, and where it plays.
fn audio_menu(outputs: Menu, block_outputs: Menu, state: &MenuState) -> Menu {
    let mut menu = items(
        &[
            "block.audio.volume_up",
            "block.audio.volume_down",
            "block.audio.mute.toggle",
            "",
            "block.audio.next_track",
            "",
            "block.audio.earlier",
            "block.audio.later",
            "block.audio.reset_delay",
            "",
            "app.audio.volume_up",
            "app.audio.volume_down",
            "app.audio.mute.toggle",
            "",
        ],
        state,
    );
    menu.extend([MenuItem::submenu(outputs), MenuItem::submenu(block_outputs)]);
    Menu {
        name: "Audio".into(),
        items: menu,
    }
}

/// The subtitles of the block in front.
fn subtitles_menu(state: &MenuState) -> Menu {
    Menu {
        name: "Subtitles".into(),
        items: items(
            &[
                "block.subtitles.toggle",
                "block.subtitles.next_track",
                "",
                "block.subtitles.bigger",
                "block.subtitles.smaller",
                "block.subtitles.up",
                "block.subtitles.down",
                "",
                "block.subtitles.earlier",
                "block.subtitles.later",
                "block.subtitles.reset_delay",
            ],
            state,
        ),
    }
}

/// Opening, naming and closing windows; maximizing, naming and moving the
/// block in front, going from block to block in the window in front and back,
/// and moving them about; and going from window to window and back, and
/// numbering them. Named so, macOS adds its own window items to it.
fn window_menu(state: &MenuState) -> Menu {
    let mut items = items(
        &[
            "window.command_palette",
            "window.go_to",
            "",
            "window.new",
            "window.rename",
            "window.close_others",
            "",
            "block.zoom",
            "block.rename",
            "block.move_to_new_window",
        ],
        state,
    );
    items.extend(place_menu(state).map(MenuItem::submenu));
    items.extend(other_window_menu(state).map(MenuItem::submenu));
    items.extend(self::items(
        &[
            "",
            "block.focus.left",
            "block.focus.right",
            "block.focus.up",
            "block.focus.down",
            "block.focus.previous",
            "block.focus.next",
            "block.back",
            "block.recent",
            "",
            "block.move.left",
            "block.move.right",
            "block.move.up",
            "block.move.down",
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
    ));
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

/// A place for each block in the window in front, the block in front's
/// checked, to put it there instead; none for a block alone.
fn place_menu(state: &MenuState) -> Option<Menu> {
    let front = state.state("block.move").as_u64();
    (state.blocks > 1).then(|| Menu {
        name: title("block.move").into(),
        items: (1..=state.blocks)
            .map(|place| {
                let checked = front == u64::try_from(place).ok();
                MenuItem::action(place.to_string(), block::move_to(place)).checked(checked)
            })
            .collect(),
    })
}

/// Each other player window, by its label or number, to move the block in
/// front to; none without one or the other.
fn other_window_menu(state: &MenuState) -> Option<Menu> {
    (state.blocks > 0 && !state.others.is_empty()).then(|| Menu {
        name: title("block.move_to_window").into(),
        items: state
            .others
            .iter()
            .map(|(name, title)| MenuItem::action(title.clone(), block::move_to_window(name)))
            .collect(),
    })
}

/// Check the menus again when Settings change (floating on top, and the
/// output picked), and the files played lately.
pub(super) fn watch_settings(cx: &mut App) {
    cx.observe_global::<Settings>(refresh).detach();
    recents::observe(cx, refresh);
}

/// What the menus show that changes without them hearing of it: besides
/// this computer's outputs, Video → Output Device and Audio → Output Device
/// show the displays, the one the window in front is on, the devices that
/// show video, the speakers, and the one the block in front casts to;
/// Playback → Chapters lists the block in front's chapters; and what each
/// command changes, for the window and block in front, checks its items.
#[derive(Clone, Default, PartialEq, Debug)]
struct MenuState {
    displays: Vec<DisplayInfo>,
    front: Option<u32>,
    devices: Vec<Device>,
    /// Those for the block in front: none when it has no sound.
    speakers: Vec<Device>,
    casting: Option<Device>,
    /// The block in front's chapters, by name.
    chapters: Vec<String>,
    /// How many player windows are open, each a number to move one to.
    windows: usize,
    /// How many blocks the window in front has, each a place to put one.
    blocks: usize,
    /// The other player windows, to move the block in front to: the API's
    /// name for each, and its label, else its number, to show.
    others: Vec<(String, String)>,
    /// The state of each command that has one, as it is for the window
    /// and block in front.
    states: BTreeMap<&'static str, Value>,
    /// The full paths of the files played lately, the latest first.
    recent_files: Vec<String>,
}

impl Global for MenuState {}

impl MenuState {
    fn now(cx: &mut App) -> Self {
        let window = front_window(cx);
        let (front, casting, has_audio, chapters, blocks) = window
            .and_then(|handle| {
                handle
                    .update(cx, |workspace, window, cx| {
                        (
                            displays::of(window, cx),
                            workspace.active_casting(cx),
                            workspace.active_has_audio(cx),
                            workspace.active_chapters(cx).0,
                            workspace.block_count(),
                        )
                    })
                    .ok()
            })
            .unwrap_or((None, None, true, Vec::new(), 0));
        let others = player_windows(cx)
            .into_iter()
            .enumerate()
            .filter(|(_, other)| window.is_none_or(|front| front.window_id() != other.window_id()))
            .filter_map(|(at, other)| {
                let workspace = other.downcast::<Workspace>()?.read(cx).ok()?;
                let title = workspace
                    .label()
                    .map_or_else(|| format!("Window {}", at + 1), str::to_owned);
                Some((workspace.id().to_string(), title))
            })
            .collect();
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
            // is made every second and as the mouse goes between blocks.
            displays: displays::connected(cx),
            front,
            devices,
            speakers,
            casting,
            chapters,
            windows: player_windows(cx).len(),
            blocks,
            others,
            states,
            recent_files: recents::FILES.list(cx),
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
/// window in front is on another display, or the block in front casts.
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
/// show has changed: from a window or block, which can't be read while
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

/// The block in front's chapters, the one it's in, `chapter` (from 1),
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

/// The aspect ratios, the block in front's, `aspect` (by name, null as the
/// file is), checked.
fn aspect_menu(aspect: &Value, state: &MenuState) -> Menu {
    Menu {
        name: title("block.video.set_aspect_ratio").into(),
        items: [(None, "As the File Is")]
            .into_iter()
            .chain(ASPECT_RATIOS.map(|r| (Some(r), r.name)))
            .map(|(ratio, name)| {
                MenuItem::action(name, video::set_aspect_ratio(ratio))
                    .checked(*aspect == named(ratio))
            })
            .chain(items(&["", "block.video.next_aspect_ratio"], state))
            .collect(),
    }
}

/// The speeds, the block in front's, `speed`, checked; then Faster,
/// Slower and Normal Speed.
fn speed_menu(speed: &Value, state: &MenuState) -> Menu {
    Menu {
        name: title("block.playback.set_speed").into(),
        items: SPEEDS
            .into_iter()
            .map(|each| {
                MenuItem::action(speed_text(each), playback::set_speed(each))
                    .checked(speed.as_f64() == Some(each))
            })
            .chain(items(
                &[
                    "",
                    "block.playback.faster",
                    "block.playback.slower",
                    "block.playback.normal_speed",
                ],
                state,
            ))
            .collect(),
    }
}

/// The crops, the block in front's, `crop` (by name, null for none),
/// checked.
fn crop_menu(crop: &Value, state: &MenuState) -> Menu {
    Menu {
        name: title("block.video.set_crop").into(),
        items: [(None, "None")]
            .into_iter()
            .chain(CROPS.map(|r| (Some(r), r.name)))
            .map(|(ratio, name)| {
                MenuItem::action(name, video::set_crop(ratio)).checked(*crop == named(ratio))
            })
            .chain(items(&["", "block.video.next_crop"], state))
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
/// devices that show video, the one the block in front casts to checked
/// instead (a speaker it casts to leaves its picture here). Picking a
/// display shows the window there, full screen; picking a device casts the
/// block in front to it.
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
/// what the sound waits for. Then the speakers, and the device the block in
/// front casts to, checked instead: picking one casts the block in front to
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

/// The app's output, checked unless the block in front plays through a
/// device of its own, then every output connected, its own checked; its
/// own is listed too while it isn't connected, as its sound waits for it.
fn block_output_menu(devices: &[DeviceInfo], own: Option<&AudioDevice>) -> Menu {
    let mut items = vec![
        MenuItem::action(output::SAME_AS_APP, output::set_block_device(None))
            .checked(own.is_none()),
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
        MenuItem::action(title, output::set_block_device(Some(&device))).checked(checked)
    }));
    if let Some(own) = own
        && !devices.iter().any(|device| device.id == own.id)
    {
        items.push(
            MenuItem::action(
                format!("{} (Not Connected)", own.name),
                output::set_block_device(Some(own)),
            )
            .checked(true),
        );
    }
    Menu {
        name: "Block Output Device".into(),
        items,
    }
}

#[cfg(test)]
#[path = "tests/menus.rs"]
mod tests;
