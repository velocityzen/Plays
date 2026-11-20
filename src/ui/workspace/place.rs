//! Where windows are: the first window of a launch opens where the last
//! one closed was, on its display and at its size; a display the system
//! numbered anew is known by its name. A window Output Device put
//! full screen on another display comes back where it was once out of
//! full screen, unless its display has gone. Wayland places windows
//! itself, whatever is asked.

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use gpui::{App, Bounds, Context, Global, Pixels, Window, WindowHandle, point, px, size};
use serde::{Deserialize, Serialize};

use super::Workspace;
use crate::settings::AudioDevice;
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
    pub(super) fn of(window: &Window, cx: &App) -> Option<Self> {
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
    /// While its sound follows the display it was sent to, what it played
    /// through before (`None` for the app's output), which it goes back to.
    pub(super) sound: Option<Option<AudioDevice>>,
}

/// The place of Plays' window in front, or of the last one closed: where
/// the next launch opens its first window.
#[derive(Default)]
struct LastPlace(Option<Place>);
impl Global for LastPlace {}

/// Where the last place is saved; nowhere in tests and smoke runs, as with
/// the settings.
fn path() -> Option<PathBuf> {
    if cfg!(any(test, feature = "smoke")) {
        return None;
    }
    dirs::config_dir().map(|dir| dir.join("plays").join("window.json"))
}

fn load() -> Option<Place> {
    let path = path()?;
    let text = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&text)
        .inspect_err(|err| log::warn!("ignoring {}: {err}", path.display()))
        .ok()
}

/// Keep `place` as the last, with its display's name, and save it.
fn keep(place: Place, cx: &mut App) {
    let name = displays::list(cx)
        .into_iter()
        .find(|display| display.id == place.display)
        .map(|display| display.name);
    let place = Place { name, ..place };
    if let Some(path) = path() {
        let result = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&path, serde_json::to_string_pretty(&place)?));
        if let Err(err) = result {
            log::warn!("could not save {}: {err}", path.display());
        }
    }
    cx.set_global(LastPlace(Some(place)));
}

/// Save the last place as the app quits.
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
        Self::open_window_where(load(), cx)
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
        let display = place.find(&displays::list(cx)).and_then(|id| {
            cx.displays()
                .into_iter()
                .find(|display| u32::from(display.id()) == id)
        });
        match display {
            Some(display) => {
                let bounds = displays::inside(place.bounds(), display.bounds());
                Self::open_window_at(Some(display.id()), bounds, cx)
            }
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
                sound: None,
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
            let Some(Home { place, sound, .. }) = self.home.take() else {
                return;
            };
            if let Some(before) = sound {
                self.play_through(before, cx);
            }
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
mod tests {
    use super::*;

    fn display(id: u32, name: &str) -> DisplayInfo {
        DisplayInfo {
            id,
            name: name.to_owned(),
            bounds: Bounds::default(),
        }
    }

    fn place(display: u32, name: Option<&str>) -> Place {
        Place {
            display,
            name: name.map(str::to_owned),
            x: 10.,
            y: 20.,
            width: 800.,
            height: 600.,
        }
    }

    #[test]
    fn finds_the_display_by_its_id_or_else_its_name() {
        let connected = [display(1, "Built-in"), display(5, "Studio")];
        assert_eq!(place(5, Some("Other")).find(&connected), Some(5), "by id");
        assert_eq!(
            place(9, Some("Studio")).find(&connected),
            Some(5),
            "by name"
        );
        assert_eq!(place(9, Some("Gone")).find(&connected), None);
        assert_eq!(place(9, None).find(&connected), None);

        let twins = [display(2, "Studio"), display(3, "Studio")];
        assert_eq!(
            place(9, Some("Studio")).find(&twins),
            None,
            "not one of two of the same name"
        );
    }

    #[test]
    fn a_place_saves_and_loads_as_it_was() {
        let saved = Place {
            name: Some("Studio".into()),
            ..place(5, None)
        };
        let text = serde_json::to_string(&saved).unwrap();
        assert_eq!(serde_json::from_str::<Place>(&text).unwrap(), saved);
        assert_eq!(saved.bounds().origin, point(px(10.), px(20.)));
    }
}
