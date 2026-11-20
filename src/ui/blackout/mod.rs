//! Settings → Appearance → Black out other displays: while a player window
//! is full screen, every other display is covered in black, for
//! projectors, TVs and dark rooms. The covers go as soon as full screen
//! ends or another app is in front. Moving the mouse on a covered display,
//! or clicking it, brings it back until the mouse has rested a moment.
//!
//! The covers are plain black windows of the system's own rather than
//! gpui's, as nothing is drawn in them and they must never take the focus:
//! on macOS above the menu bar and Dock, on every Space; on X11 one per
//! monitor (RandR), which the window manager leaves alone. Wayland lets no
//! app place windows, so there it does nothing.

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

/// What a cover can go over: a display, or on X11 a monitor of one.
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
    /// Whether `spot` is on it. A screen has its left and top edges, and
    /// leaves its right and bottom ones to its neighbors.
    fn has(&self, spot: &Spot) -> bool {
        let bounds = &self.bounds;
        self.display == spot.display
            && spot.at.is_none_or(|at| {
                (bounds.left()..bounds.right()).contains(&at.x)
                    && (bounds.top()..bounds.bottom()).contains(&at.y)
            })
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
        let screens = blackout.native.screens(displays, scale);
        let shown = blackout
            .peek
            .showing(blackout.native.mouse(&screens, scale), Instant::now());
        to_cover(&screens, &full_screen, shown)
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

/// gpui's displays, each a screen to cover.
fn displays(cx: &App) -> Vec<Screen> {
    cx.displays()
        .iter()
        .map(|display| Screen {
            display: display.id().into(),
            bounds: display.bounds(),
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
    fn screens(&mut self, displays: Vec<Screen>, _: f32) -> Vec<Screen> {
        displays
    }

    fn mouse(&mut self, _: &[Screen], _: f32) -> Option<Mouse> {
        None
    }

    fn show(&mut self, _: &[Screen], _: f32) {}
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui::{point, px, size};

    use super::*;

    fn screen(display: u32, x: f32) -> Screen {
        Screen {
            display,
            bounds: Bounds::new(point(px(x), px(0.)), size(px(1000.), px(800.))),
        }
    }

    fn on(display: u32, x: f32) -> Spot {
        Spot {
            display,
            at: Some(point(px(x), px(400.))),
        }
    }

    #[test]
    fn covers_every_screen_but_those_full_screen_and_the_one_the_mouse_shows() {
        let screens = [screen(1, 0.), screen(2, 0.), screen(3, 0.)];
        let displays = |covered: Vec<Screen>| -> Vec<u32> {
            covered.iter().map(|screen| screen.display).collect()
        };
        assert_eq!(
            displays(to_cover(&screens, &[], None)),
            [0u32; 0],
            "none full screen"
        );
        assert_eq!(displays(to_cover(&screens, &[on(1, 500.)], None)), [2, 3]);
        assert_eq!(
            displays(to_cover(&screens, &[on(1, 500.), on(3, 500.)], None)),
            [2],
            "full screen on two"
        );
        let mouse = Spot {
            display: 2,
            at: None,
        };
        assert_eq!(
            displays(to_cover(&screens, &[on(1, 500.)], Some(mouse))),
            [3],
            "the mouse shows the one it is on"
        );

        // X11: monitors side by side on one display.
        let monitors = [screen(0, 0.), screen(0, 1000.)];
        let covered = to_cover(&monitors, &[on(0, 1500.)], None);
        assert_eq!(covered, [screen(0, 0.)], "the monitor without it");
        let covered = to_cover(&monitors, &[on(0, 1000.)], None);
        assert_eq!(
            covered,
            [screen(0, 0.)],
            "a window over both, centered on their edge, is on the one right of it"
        );
    }

    #[test]
    fn the_mouse_shows_a_screen_from_moving_or_clicking_until_it_rests() {
        let mut peek = Peek::default();
        let start = Instant::now();
        let mouse = |x: f64, pressed: bool| Mouse {
            spot: Spot {
                display: 2,
                at: None,
            },
            at: (x, 0.),
            pressed,
        };
        let later = |seconds: f64| start + Duration::from_secs_f64(seconds);

        assert_eq!(peek.showing(None, start), None, "where, unknown");
        assert_eq!(peek.showing(Some(mouse(0., false)), start), None, "resting");
        assert!(
            peek.showing(Some(mouse(5., false)), later(0.1)).is_some(),
            "moved"
        );
        let rested = later(0.1) + BLACKOUT_PEEK;
        assert!(peek.showing(Some(mouse(5., false)), later(0.2)).is_some());
        assert_eq!(peek.showing(Some(mouse(5., false)), rested), None, "rested");
        assert!(
            peek.showing(Some(mouse(5., true)), rested).is_some(),
            "clicked"
        );
    }
}
