//! The displays a player window can show on, for Video → Output Device, and
//! where new windows open. On macOS an AirPlay display is one of them once
//! it is connected (Control Center → Screen Mirroring, as a separate
//! display), and macOS sends the sound to it too. gpui has no names for
//! displays and can't move windows, so on macOS this goes to AppKit;
//! elsewhere displays are numbered and the window manager places windows.
//!
//! New windows open on the display in use, not always on the main one: a
//! tile's own window beside the window it left, and others on the display
//! of Plays' window in front, or with the mouse when none is. A window's
//! bounds are relative to its display, as gpui opens and reports them.

use gpui::{App, Bounds, DisplayId, Pixels, Size, Window, point};

use crate::ui::theme::layout::WINDOW_GAP;

/// A display, as the system names it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DisplayInfo {
    /// gpui's id for it: on macOS, its `CGDirectDisplayID`.
    pub id: u32,
    pub name: String,
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
            let name = names.iter().find(|(screen, _)| *screen == id).map_or_else(
                || format!("Display {}", index + 1),
                |(_, name)| name.clone(),
            );
            DisplayInfo { id, name }
        })
        .collect()
}

/// The display `window` is on.
pub fn of(window: &Window, cx: &App) -> Option<u32> {
    window.display(cx).map(|display| u32::from(display.id()))
}

/// Move `window` onto display `id`, centered, at its size or as much of it
/// as fits. Only on macOS, and not for a window that is full screen.
pub fn move_to(window: &Window, id: u32) {
    #[cfg(target_os = "macos")]
    mac::move_to(window, id);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, id);
}

/// Put `window` at `bounds` on display `id`, relative to it, as gpui opens
/// windows there. Only on macOS, and not for a window that is full screen.
pub fn put(window: &Window, id: u32, bounds: Bounds<Pixels>) {
    #[cfg(target_os = "macos")]
    mac::put(window, id, bounds);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, id, bounds);
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
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::beside;
    use crate::ui::theme::layout::WINDOW_GAP;

    #[test]
    fn a_window_opens_beside_the_one_it_came_from() {
        let room = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
        let new = size(px(600.), px(400.));
        let at = |x: f32, y: f32, width: f32| {
            Bounds::new(point(px(x), px(y)), size(px(width), px(500.)))
        };

        let right = beside(at(100., 100., 800.), new, room);
        assert_eq!(
            right.origin,
            point(px(900.) + WINDOW_GAP, px(100.)),
            "to the right"
        );
        assert_eq!(right.size, new);

        let left = beside(at(1000., 100., 800.), new, room);
        assert_eq!(
            left.origin.x,
            px(400.) - WINDOW_GAP,
            "to the left, with no room right"
        );

        let middle = beside(at(300., 100., 1400.), new, room);
        assert_eq!(
            middle.origin.x,
            px(660.),
            "centered, with room on neither side"
        );

        let low = beside(at(100., 900., 800.), new, room);
        assert_eq!(
            low.origin.y,
            px(680.),
            "up from the bottom, to stay on the display"
        );

        let wide = beside(at(100., 100., 800.), size(px(2400.), px(400.)), room);
        assert_eq!(
            wide.origin.x,
            px(0.),
            "its left edge on it, when it's wider than that"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn appkit_is_asked_the_screens_names_on_the_main_thread_only() {
        // Tests run off it, as this one does.
        assert!(super::mac::names().is_empty());
        assert_eq!(super::mac::with_mouse(), None);
    }
}
