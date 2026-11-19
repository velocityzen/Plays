//! The displays a player window can show on, for Video → Output Device. On
//! macOS an AirPlay display is one of them once it is connected (Control
//! Center → Screen Mirroring, as a separate display), and macOS sends the
//! sound to it too. gpui has no names for displays and can't move windows,
//! so on macOS this goes to AppKit; elsewhere displays are numbered and the
//! window manager places windows.

use gpui::{App, Window};

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

// objc's message macros test a `cargo-clippy` feature this crate doesn't have.
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod mac {
    use std::ffi::{CStr, c_char};

    use gpui::Window;
    use objc::runtime::{Object, YES};
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

    /// Each screen's `CGDirectDisplayID` and its name.
    pub fn names() -> Vec<(u32, String)> {
        // SAFETY: AppKit on the main thread, where gpui runs the app; the
        // screens and strings are autoreleased objects read right away.
        unsafe {
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

    pub fn move_to(window: &Window, id: u32) {
        // gpui's own `Window::window_handle` returns something else.
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return;
        };
        let view = handle.ns_view.as_ptr().cast::<Object>();
        // SAFETY: `view` is the window's NSView, alive as long as `window`,
        // on the main thread.
        unsafe {
            let ns_window: *mut Object = msg_send![view, window];
            if ns_window.is_null() {
                return;
            }
            let screens: *mut Object = msg_send![class!(NSScreen), screens];
            let count: usize = msg_send![screens, count];
            let Some(screen) = (0..count)
                .map(|index| -> *mut Object { msg_send![screens, objectAtIndex: index] })
                .find(|&screen| number(screen) == Some(id))
            else {
                return;
            };
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
}
