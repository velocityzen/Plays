//! The displays a player window can show on, for Output Device (Video →
//! Output Device, and the block's cast menu), and where new windows open. On
//! macOS an AirPlay display is one of them once it is connected (Control
//! Center → Screen Mirroring, as a separate display), and macOS sends the
//! sound to it too; names and moves there go to AppKit. On X11 each monitor
//! is a display, by its product's name (EDID), and gpui moves windows; on
//! Wayland a display is an output, by the compositor's description of it,
//! and the compositor places windows, so a window goes full screen on one
//! only as far as it agrees.
//!
//! New windows open on the display in use, not always on the main one: a
//! block's own window beside the window it left, and others on the display
//! of Plays' window in front, or with the mouse when none is. A window's
//! bounds are relative to its display, as gpui opens and reports them.

use gpui::{App, Bounds, DisplayId, Global, Pixels, Size, Window, point};

use crate::ui::theme::layout::WINDOW_GAP;
use crate::ui::theme::motion::DISPLAYS_POLL;

/// A display, as the system names it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DisplayInfo {
    /// gpui's id for it: on macOS, its `CGDirectDisplayID`.
    pub id: u32,
    pub name: String,
    /// Where it is, as gpui has displays' bounds.
    pub bounds: Bounds<Pixels>,
}

/// The displays connected, in the system's order.
pub fn list(cx: &App) -> Vec<DisplayInfo> {
    #[cfg(target_os = "macos")]
    let names = mac::names();
    #[cfg(not(target_os = "macos"))]
    let names: Vec<(u32, String)> = Vec::new();
    cx.displays()
        .iter()
        .enumerate()
        .map(|(index, display)| {
            let id = u32::from(display.id());
            let name = names
                .iter()
                .find(|(screen, _)| *screen == id)
                .map(|(_, name)| name.clone())
                .or_else(|| display.name())
                .unwrap_or_else(|| format!("Display {}", index + 1));
            DisplayInfo {
                id,
                name,
                bounds: display.bounds(),
            }
        })
        .collect()
}

/// The displays connected when last looked (`watch`), as asking for them is
/// too slow on X11 (a few round trips to the X server for each) to do for
/// every frame drawn, or as often as Black out looks where they are.
struct Connected(Vec<DisplayInfo>);
impl Global for Connected {}

/// The displays connected, as last looked (`watch`); looked at now where
/// nothing watches them, as in tests.
pub fn connected(cx: &App) -> Vec<DisplayInfo> {
    cx.try_global::<Connected>()
        .map_or_else(|| list(cx), |connected| connected.0.clone())
}

/// Take `displays` as the ones last looked at, as `watch` does.
#[cfg(test)]
pub fn remember(displays: Vec<DisplayInfo>, cx: &mut App) {
    cx.set_global(Connected(displays));
}

/// Look at the displays every `DISPLAYS_POLL`, and draw the windows again
/// when one comes or goes.
pub fn watch(cx: &mut App) {
    let now = list(cx);
    cx.set_global(Connected(now));
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(DISPLAYS_POLL).await;
            let looked = cx.update(|cx| {
                let now = list(cx);
                if cx.global::<Connected>().0 != now {
                    cx.set_global(Connected(now));
                    cx.refresh_windows();
                }
            });
            if looked.is_err() {
                break;
            }
        }
    })
    .detach();
}

/// gpui's id for display `id`, while it is connected.
pub fn find(id: u32, cx: &App) -> Option<DisplayId> {
    cx.displays()
        .into_iter()
        .map(|display| display.id())
        .find(|&found| u32::from(found) == id)
}

/// The display `window` is on.
pub fn of(window: &Window, cx: &App) -> Option<u32> {
    window.display(cx).map(|display| u32::from(display.id()))
}

/// Put `window`, out of full screen, full screen on display `id`. macOS
/// goes full screen where a window is, so it moves there first, centered,
/// at its size or as much of it as fits; on X11 gpui moves it, and Wayland
/// asks the compositor, which may keep it where it is.
pub fn full_screen_on(window: &Window, id: u32, cx: &App) {
    #[cfg(target_os = "macos")]
    mac::move_to(window, id);
    match find(id, cx) {
        Some(display) => window.fullscreen_on(display),
        None if !window.is_fullscreen() => window.toggle_fullscreen(),
        None => {}
    }
}

/// Put `window` at `bounds` on display `id`, as gpui opens windows there.
/// Not on Wayland, which places windows itself, and not for a window that
/// is full screen.
pub fn put(window: &Window, id: u32, bounds: Bounds<Pixels>) {
    #[cfg(target_os = "macos")]
    mac::put(window, id, bounds);
    // A display's bounds and a window's are both the screen's on X11.
    #[cfg(not(target_os = "macos"))]
    {
        let _ = id;
        window.set_frame(bounds);
    }
}

/// Whether the compositor places windows, rather than Plays: on Wayland,
/// where gpui runs when there is a Wayland display.
pub fn placed_by_compositor() -> bool {
    #[cfg(target_os = "linux")]
    return gpui::guess_compositor() == "Wayland";
    #[cfg(not(target_os = "linux"))]
    false
}

/// The display a new window opens on: the one with Plays' window in front,
/// or else, on macOS, the one with the mouse. `None` leaves it to the
/// system, which opens it on the main display. Ask it once whatever asked
/// for the window is done with its own (`App::defer`), or the window in
/// front can't say where it is.
pub fn in_use(cx: &mut App) -> Option<DisplayId> {
    let front = cx.active_window().and_then(|handle| {
        handle
            .update(cx, |_, window, cx| {
                window.display(cx).map(|display| display.id())
            })
            .ok()
            .flatten()
    });
    front.or_else(|| {
        let mouse = with_mouse()?;
        cx.displays()
            .into_iter()
            .map(|display| display.id())
            .find(|&id| u32::from(id) == mouse)
    })
}

/// The display with the mouse, as the system numbers it; only macOS says.
fn with_mouse() -> Option<u32> {
    #[cfg(target_os = "macos")]
    return mac::with_mouse();
    #[cfg(not(target_os = "macos"))]
    None
}

/// Where a window of `size` goes beside the window at `from` on a display
/// of `room` (bounds relative to the display): to its right, or else to its
/// left, the tops level, or centered where neither side has room. On the
/// display, as far as it fits.
pub fn beside(from: Bounds<Pixels>, size: Size<Pixels>, room: Bounds<Pixels>) -> Bounds<Pixels> {
    let right = from.right() + WINDOW_GAP;
    let left = from.left() - WINDOW_GAP - size.width;
    let x = if right + size.width <= room.right() {
        right
    } else if left >= room.left() {
        left
    } else {
        room.center().x - size.width / 2.
    };
    inside(Bounds::new(point(x, from.top()), size), room)
}

/// `bounds` moved onto a display of `room`, and made smaller where it is
/// bigger than that.
pub fn inside(bounds: Bounds<Pixels>, room: Bounds<Pixels>) -> Bounds<Pixels> {
    let size = bounds.size.min(&room.size);
    let x = bounds
        .left()
        .min(room.right() - size.width)
        .max(room.left());
    let y = bounds
        .top()
        .min(room.bottom() - size.height)
        .max(room.top());
    Bounds::new(point(x, y), size)
}

// objc's message macros test a `cargo-clippy` feature this crate doesn't have.
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod mac {
    use std::ffi::{CStr, c_char};

    use gpui::{Bounds, Pixels, Window};
    use objc::runtime::{BOOL, Object, YES};
    use objc::{class, msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }

    /// The `CGDirectDisplayID` of the screen with the mouse, asked on the
    /// main thread only, as `names` is.
    pub fn with_mouse() -> Option<u32> {
        // SAFETY: as in `names`; the point and frames are plain structs,
        // both in AppKit's global coordinates.
        unsafe {
            let main: BOOL = msg_send![class!(NSThread), isMainThread];
            if main != YES {
                return None;
            }
            let mouse: Point = msg_send![class!(NSEvent), mouseLocation];
            let screens: *mut Object = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            (0..count)
                .map(|index| -> *mut Object { msg_send![screens, objectAtIndex: index] })
                .find(|&screen| {
                    let frame: Rect = msg_send![screen, frame];
                    (frame.x..frame.x + frame.width).contains(&mouse.x)
                        && (frame.y..frame.y + frame.height).contains(&mouse.y)
                })
                .and_then(|screen| number(screen))
        }
    }

    /// Each screen's `CGDirectDisplayID` and its name, asked on the main
    /// thread only, where gpui runs the app. Tests run on threads of their
    /// own, many at once, and AppKit asked from them at once breaks its
    /// heap; their displays aren't the screens anyway.
    pub fn names() -> Vec<(u32, String)> {
        // SAFETY: NSThread says which thread this is from any; the rest is
        // AppKit on the main thread, and the screens and strings are
        // autoreleased objects read right away.
        unsafe {
            let main: BOOL = msg_send![class!(NSThread), isMainThread];
            if main != YES {
                return Vec::new();
            }
            let screens: *mut Object = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            (0..count)
                .filter_map(|index| {
                    let screen: *mut Object = msg_send![screens, objectAtIndex: index];
                    let name: *mut Object = msg_send![screen, localizedName];
                    Some((number(screen)?, string(name)?))
                })
                .collect()
        }
    }

    /// The screen's `CGDirectDisplayID`.
    unsafe fn number(screen: *mut Object) -> Option<u32> {
        // SAFETY: as in `names`.
        unsafe {
            let key: *mut Object =
                msg_send![class!(NSString), stringWithUTF8String: c"NSScreenNumber".as_ptr()];
            let description: *mut Object = msg_send![screen, deviceDescription];
            let number: *mut Object = msg_send![description, objectForKey: key];
            if number.is_null() {
                return None;
            }
            let id: u32 = msg_send![number, unsignedIntValue];
            Some(id)
        }
    }

    unsafe fn string(string: *mut Object) -> Option<String> {
        if string.is_null() {
            return None;
        }
        // SAFETY: an NSString's UTF-8 lives as long as it does.
        unsafe {
            let utf8: *const c_char = msg_send![string, UTF8String];
            (!utf8.is_null()).then(|| CStr::from_ptr(utf8).to_string_lossy().into_owned())
        }
    }

    /// The NSWindow of `window`.
    fn ns_window(window: &Window) -> Option<*mut Object> {
        // gpui's own `Window::window_handle` returns something else.
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return None;
        };
        let view = handle.ns_view.as_ptr().cast::<Object>();
        // SAFETY: `view` is the window's NSView, alive as long as `window`,
        // on the main thread.
        let ns_window: *mut Object = unsafe { msg_send![view, window] };
        (!ns_window.is_null()).then_some(ns_window)
    }

    /// The NSScreen of display `id`.
    fn screen(id: u32) -> Option<*mut Object> {
        // SAFETY: as in `names`.
        unsafe {
            let screens: *mut Object = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            (0..count)
                .map(|index| -> *mut Object { msg_send![screens, objectAtIndex: index] })
                .find(|&screen| number(screen) == Some(id))
        }
    }

    pub fn move_to(window: &Window, id: u32) {
        let (Some(ns_window), Some(screen)) = (ns_window(window), screen(id)) else {
            return;
        };
        // SAFETY: both are alive while `window` is, on the main thread.
        unsafe {
            let area: Rect = msg_send![screen, visibleFrame];
            let frame: Rect = msg_send![ns_window, frame];
            let (width, height) = (frame.width.min(area.width), frame.height.min(area.height));
            let moved = Rect {
                x: area.x + (area.width - width) / 2.0,
                y: area.y + (area.height - height) / 2.0,
                width,
                height,
            };
            let _: () = msg_send![ns_window, setFrame: moved display: YES];
        }
    }

    pub fn put(window: &Window, id: u32, bounds: Bounds<Pixels>) {
        let (Some(ns_window), Some(screen)) = (ns_window(window), screen(id)) else {
            return;
        };
        // SAFETY: as in `move_to`. AppKit's frames start at the bottom left
        // of the main screen; gpui's bounds at the top left of their own.
        unsafe {
            let display: Rect = msg_send![screen, frame];
            let (width, height) = (f64::from(bounds.size.width), f64::from(bounds.size.height));
            let frame = Rect {
                x: display.x + f64::from(bounds.origin.x),
                y: display.y + display.height - f64::from(bounds.origin.y) - height,
                width,
                height,
            };
            let _: () = msg_send![ns_window, setFrame: frame display: YES];
        }
    }
}

#[cfg(test)]
#[path = "tests/displays.rs"]
mod tests;
