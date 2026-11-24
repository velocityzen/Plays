//! Where windows are: the first window of a launch opens where the last
//! one closed was, on its display and at its size, and Settings where it
//! was last, this launch or one before; a display the system numbered anew
//! is known by its name. A window Output Device put full screen on another
//! display comes back where it was once out of full screen, unless its
//! display has gone. Wayland places windows itself, whatever is asked.

use std::fs;

use anyhow::Result;
use gpui::{
    App, Bounds, Context, DisplayId, Global, Pixels, Window, WindowHandle, point, px, size,
};
use serde::{Deserialize, Serialize};

use super::Workspace;
use crate::files::{self, Folder};
use crate::ui::displays::{self, DisplayInfo};
use crate::ui::theme::layout::WINDOW_SIZE;

/// Where a window was: its display, and its bounds there.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Place {
    /// The display's id, as gpui has it: on macOS, its `CGDirectDisplayID`.
    pub display: u32,
    /// The display's name, to know it by when its id has changed.
    #[serde(default)]
    pub name: Option<String>,
    /// The window's bounds, relative to the display.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Place {
    /// Where `window` is, out of full screen.
    pub(crate) fn of(window: &Window, cx: &App) -> Option<Self> {
        let display = displays::of(window, cx)?;
        let bounds = window.window_bounds().get_bounds();
        Some(Self {
            display,
            name: None,
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
        })
    }

    pub fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(
            point(px(self.x), px(self.y)),
            size(px(self.width), px(self.height)),
        )
    }

    /// Where a window opens at this place: on its display, or the one with
    /// its name, as much of it as fits; `None` with that display gone.
    pub(crate) fn on_its_display(&self, cx: &App) -> Option<(DisplayId, Bounds<Pixels>)> {
        let id = self.find(&displays::list(cx))?;
        let display = cx
            .displays()
            .into_iter()
            .find(|display| u32::from(display.id()) == id)?;
        let bounds = displays::inside(self.bounds(), display.bounds());
        Some((display.id(), bounds))
    }

    /// Its display among those connected: the one with its id, or else the
    /// only one with its name.
    pub fn find(&self, connected: &[DisplayInfo]) -> Option<u32> {
        if connected.iter().any(|display| display.id == self.display) {
            return Some(self.display);
        }
        let name = self.name.as_deref()?;
        let mut named = connected.iter().filter(|display| display.name == name);
        match (named.next(), named.next()) {
            (Some(display), None) => Some(display.id),
            _ => None,
        }
    }
}

/// Where a window was before Output Device put it full screen on another
/// display.
pub(super) struct Home {
    place: Place,
    /// It has been full screen there since, so leaving full screen is the
    /// way back.
    pub(super) shown: bool,
}

/// The place of Plays' window in front, or of the last one closed: where
/// the next launch opens its first window.
#[derive(Default)]
struct LastPlace(Option<Place>);
impl Global for LastPlace {}

/// The file the player window's last place is saved in.
const PLAYER_FILE: &str = "window.json";
/// The file Settings' place is saved in.
const SETTINGS_FILE: &str = "settings-window.json";

/// Where Settings was last, this launch or one before: where it opens
/// next. Read from its file the first time it's asked for.
struct SettingsPlace(Option<Place>);
impl Global for SettingsPlace {}

/// The place saved in `file`, in the app's folder (none in tests and smoke
/// runs, as with the settings, unless a test gives one).
fn load(file: &str, cx: &App) -> Option<Place> {
    let path = Folder::of(cx).file(file)?;
    let text = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text)
        .inspect_err(|err| log::warn!("ignoring {}: {err}", path.display()))
        .ok()
}

fn save(file: &str, place: &Place, cx: &App) {
    let Some(path) = Folder::of(cx).file(file) else {
        return;
    };
    let result = serde_json::to_vec_pretty(place)
        .map_err(std::io::Error::other)
        .and_then(|text| files::write_whole(&path, &text));
    if let Err(err) = result {
        log::warn!("could not save {}: {err}", path.display());
    }
}

/// `place` with its display's name, to know the display by when the
/// system numbers it anew: as last looked at (`displays::connected`), as
/// asking the system on every move is slow on X11.
fn named(place: Place, cx: &App) -> Place {
    let name = displays::connected(cx)
        .into_iter()
        .find(|display| display.id == place.display)
        .map(|display| display.name);
    Place { name, ..place }
}

/// Keep `place` as the last, with its display's name, and save it.
fn keep(place: Place, cx: &mut App) {
    let place = named(place, cx);
    save(PLAYER_FILE, &place, cx);
    cx.set_global(LastPlace(Some(place)));
}

/// Where Settings opens: where it was last, if it has been open before.
pub fn settings_place(cx: &mut App) -> Option<Place> {
    if !cx.has_global::<SettingsPlace>() {
        let saved = load(SETTINGS_FILE, cx);
        cx.set_global(SettingsPlace(saved));
    }
    cx.global::<SettingsPlace>().0.clone()
}

/// Note where the Settings `window` is, for it to open there next.
pub fn note_settings_place(window: &Window, cx: &mut App) {
    if let Some(place) = Place::of(window, cx) {
        let place = named(place, cx);
        cx.set_global(SettingsPlace(Some(place)));
    }
}

/// Save where Settings was last, for the next launch: as it closes.
pub fn save_settings_place(cx: &App) {
    if let Some(place) = cx
        .try_global::<SettingsPlace>()
        .and_then(|kept| kept.0.as_ref())
    {
        save(SETTINGS_FILE, place, cx);
    }
}

/// Forget where Settings was this launch, as the next launch starts: it
/// reads its file again.
#[cfg(test)]
pub fn forget_settings_place(cx: &mut App) {
    if cx.has_global::<SettingsPlace>() {
        cx.remove_global::<SettingsPlace>();
    }
}

/// Save the last place as the app quits. (Settings saves its own as its
/// window closes, which quitting does too.)
pub fn save_place_on_quit(cx: &mut App) {
    cx.on_app_quit(|cx| {
        if let Some(place) = cx.try_global::<LastPlace>().and_then(|last| last.0.clone()) {
            keep(place, cx);
        }
        async {}
    })
    .detach();
}

impl Workspace {
    /// Open the first window of a launch where the last one closed was
    /// (`load`), or as a new window opens when there's none or its display
    /// is gone, at its size.
    pub fn open_first_window(cx: &mut App) -> Result<WindowHandle<Self>> {
        Self::open_window_where(load(PLAYER_FILE, cx), cx)
    }

    /// Open a window at `place`, on its display and as much of it as fits;
    /// centered on the display in use, at its size, when its display is
    /// gone; or as a new window opens, without one.
    pub(crate) fn open_window_where(
        place: Option<Place>,
        cx: &mut App,
    ) -> Result<WindowHandle<Self>> {
        let Some(place) = place else {
            return Self::open_window(WINDOW_SIZE, cx);
        };
        match place.on_its_display(cx) {
            Some((display, bounds)) => Self::open_window_at(Some(display), bounds, cx),
            None => Self::open_window(place.bounds().size, cx),
        }
    }

    /// Note where the window is, unless it is away full screen on another
    /// display; while it is in front, that is where the next launch opens.
    pub(super) fn note_place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.home.is_some() {
            return;
        }
        self.place = Place::of(window, cx);
        if window.is_window_active() {
            cx.set_global(LastPlace(self.place.clone()));
        }
    }

    /// The window closed: the next launch opens where it was.
    pub(super) fn closed(&mut self, cx: &mut App) {
        let place = self
            .home
            .take()
            .map(|home| home.place)
            .or(self.place.take());
        if let Some(place) = place {
            keep(place, cx);
        }
    }

    /// Output Device is moving the window to another display: remember
    /// where it was, unless it is away already.
    pub(super) fn leave_home(&mut self) {
        if self.home.is_none() {
            self.home = self.place.clone().map(|place| Home {
                place,
                shown: false,
            });
        }
    }

    /// Where the next launch would open its first window.
    #[cfg(test)]
    pub(super) fn last_place(cx: &App) -> Option<Place> {
        cx.try_global::<LastPlace>().and_then(|last| last.0.clone())
    }

    /// Whether the window is away full screen on another display.
    #[cfg(test)]
    pub(super) fn is_away(&self) -> bool {
        self.home.is_some()
    }

    /// Follow the window in and out of full screen once Output Device moved
    /// it: in full screen, it has been shown there; moved again, it hasn't
    /// yet; out of full screen after it was, it goes back where it was, on
    /// its display or one of its name, as much of it as fits, and its sound
    /// to what it played through before. With that display gone, it stays
    /// where the system put it. Runs on every render and tick, after
    /// `finish_moving`.
    pub(super) fn come_home(&mut self, window: &mut Window, moved: bool, cx: &mut Context<Self>) {
        let Some(home) = &mut self.home else {
            return;
        };
        if window.is_fullscreen() {
            home.shown = true;
        } else if moved {
            home.shown = false;
        } else if home.shown {
            let Some(Home { place, .. }) = self.home.take() else {
                return;
            };
            self.play_through_display(None, cx);
            let display = place
                .find(&displays::list(cx))
                .and_then(|id| cx.find_display(displays::find(id, cx)?));
            if let Some(display) = display {
                let id = u32::from(display.id());
                displays::put(
                    window,
                    id,
                    displays::inside(place.bounds(), display.bounds()),
                );
                self.place = Some(Place {
                    display: id,
                    ..place
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/place.rs"]
mod tests;
