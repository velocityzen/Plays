//! Settings → Appearance → Black out other displays: while a player window
//! is full screen, every other display is covered in black, for
//! projectors, TVs and dark rooms. The covers go as soon as full screen
//! ends or another app is in front. Moving the mouse on a covered display,
//! or clicking it, brings it back until the mouse has rested a moment.
//!
//! The covers are plain black windows of the system's own rather than
//! gpui's, as nothing is drawn in them and they must never take the focus:
//! on macOS above the menu bar and Dock, on every Space; on X11 one per
//! monitor, which gpui counts as a display, that the window manager leaves
//! alone. Wayland lets no app place windows, so there it does nothing.

#[cfg(all(target_os = "macos", not(test)))]
mod mac;
#[cfg(all(target_os = "linux", not(test)))]
mod x11;

use std::time::Instant;

use gpui::{App, Bounds, Global, Pixels, Point};

use crate::settings::Settings;
use crate::ui::displays;
use crate::ui::theme::motion::{BLACKOUT_PEEK, BLACKOUT_POLL};
use crate::ui::workspace::Workspace;

/// What a cover can go over: a display.
#[derive(Clone, PartialEq, Debug)]
pub struct Screen {
    /// The display it is on, as gpui numbers them.
    pub display: u32,
    /// Where it is on the display, as gpui has bounds there.
    pub bounds: Bounds<Pixels>,
}

/// Somewhere on a display: at a point, where that is known.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Spot {
    pub display: u32,
    pub at: Option<Point<Pixels>>,
}

impl Screen {
    /// Whether `spot` is on it.
    fn has(&self, spot: &Spot) -> bool {
        self.display == spot.display && spot.at.is_none_or(|at| self.has_point(at))
    }

    /// Whether `at` is on it. A screen has its left and top edges, and
    /// leaves its right and bottom ones to its neighbors.
    pub fn has_point(&self, at: Point<Pixels>) -> bool {
        let bounds = &self.bounds;
        (bounds.left()..bounds.right()).contains(&at.x)
            && (bounds.top()..bounds.bottom()).contains(&at.y)
    }
}

/// The screens to cover: every one but those with a window full screen on
/// them, and the one the mouse is showing; none while no window is.
pub fn to_cover(screens: &[Screen], full_screen: &[Spot], shown: Option<Spot>) -> Vec<Screen> {
    if full_screen.is_empty() {
        return Vec::new();
    }
    screens
        .iter()
        .filter(|screen| {
            !full_screen.iter().any(|spot| screen.has(spot))
                && !shown.is_some_and(|spot| screen.has(&spot))
        })
        .cloned()
        .collect()
}

/// Where the mouse is, as the system says.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Mouse {
    /// The display it is on, and where, as gpui has it.
    pub spot: Spot,
    /// Where it is on the whole desktop, in the system's own terms, to tell
    /// it moved.
    pub at: (f64, f64),
    /// A button is down.
    pub pressed: bool,
}

/// Which screen the mouse shows: the one it is on, from when it moves or
/// clicks there until it has rested `BLACKOUT_PEEK`.
#[derive(Default)]
struct Peek {
    last: Option<(f64, f64)>,
    stirred: Option<Instant>,
}

impl Peek {
    fn showing(&mut self, mouse: Option<Mouse>, now: Instant) -> Option<Spot> {
        let mouse = mouse?;
        if mouse.pressed || self.last.is_some_and(|last| last != mouse.at) {
            self.stirred = Some(now);
        }
        self.last = Some(mouse.at);
        self.stirred
            .is_some_and(|stirred| now - stirred < BLACKOUT_PEEK)
            .then_some(mouse.spot)
    }
}

/// The covers up, and how to put them up.
#[derive(Default)]
struct Blackout {
    covered: Vec<Screen>,
    peek: Peek,
    native: Native,
}

impl Global for Blackout {}

/// Look, every `BLACKOUT_POLL`, at what to cover.
pub fn watch(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(BLACKOUT_POLL).await;
            if cx.update(update).is_err() {
                break;
            }
        }
    })
    .detach();
}

/// Cover the screens to cover now, and only those.
pub fn update(cx: &mut App) {
    let (full_screen, scale) =
        if cx.global::<Settings>().black_out_displays && cx.active_window().is_some() {
            full_screen(cx)
        } else {
            (Vec::new(), 1.)
        };
    let displays = if full_screen.is_empty() {
        Vec::new()
    } else {
        displays(cx)
    };
    let blackout = cx.default_global::<Blackout>();
    let wanted = if full_screen.is_empty() {
        blackout.peek = Peek::default();
        Vec::new()
    } else {
        let shown = blackout
            .peek
            .showing(blackout.native.mouse(&displays, scale), Instant::now());
        to_cover(&displays, &full_screen, shown)
    };
    if wanted != blackout.covered {
        blackout.native.show(&wanted, scale);
        blackout.covered = wanted;
    }
}

/// The screens covered now.
#[cfg(test)]
pub fn covered(cx: &App) -> Vec<Screen> {
    cx.try_global::<Blackout>()
        .map(|blackout| blackout.covered.clone())
        .unwrap_or_default()
}

/// Where Plays' windows full screen are, and the scale of the first,
/// which on X11 is every window's.
fn full_screen(cx: &mut App) -> (Vec<Spot>, f32) {
    let mut scale = 1.;
    let spots = cx
        .windows()
        .into_iter()
        .filter_map(|handle| handle.downcast::<Workspace>())
        .filter_map(|handle| {
            handle
                .update(cx, |_, window, cx| {
                    if !window.is_fullscreen() {
                        return None;
                    }
                    scale = window.scale_factor();
                    Some(Spot {
                        display: displays::of(window, cx)?,
                        at: Some(window.bounds().center()),
                    })
                })
                .ok()
                .flatten()
        })
        .collect();
    (spots, scale)
}

/// The displays, each a screen to cover, as last looked at
/// (`displays::connected`): asking the system ten times a second is too
/// slow on X11.
fn displays(cx: &App) -> Vec<Screen> {
    displays::connected(cx)
        .into_iter()
        .map(|display| Screen {
            display: display.id,
            bounds: display.bounds,
        })
        .collect()
}

#[cfg(all(target_os = "macos", not(test)))]
use mac::Covers as Native;
#[cfg(all(target_os = "linux", not(test)))]
use x11::Covers as Native;

/// Tests, and systems without covers, put none up.
#[cfg(any(test, not(any(target_os = "macos", target_os = "linux"))))]
#[derive(Default)]
struct Native;

#[cfg(any(test, not(any(target_os = "macos", target_os = "linux"))))]
impl Native {
    fn mouse(&mut self, _: &[Screen], _: f32) -> Option<Mouse> {
        None
    }

    fn show(&mut self, _: &[Screen], _: f32) {}
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
