//! The covers on macOS: borderless black windows over each screen's whole
//! frame, above the menu bar and Dock, on every Space, which never become
//! key. AppKit on the main thread, where gpui runs the app.

// objc's message macros test a `cargo-clippy` feature this crate doesn't have.
#![allow(unexpected_cfgs)]

use objc::runtime::{BOOL, NO, Object, YES};
use objc::{class, msg_send, sel, sel_impl};

use super::{Mouse, Screen, Spot};

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

/// Above the menu bar (24) and the Dock (20): `NSStatusWindowLevel`.
const LEVEL: isize = 25;
/// On every Space, staying put when they change, left out of ⌘` and
/// allowed beside a window full screen in its own Space.
const BEHAVIOR: usize = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
const BORDERLESS: usize = 0;
const BUFFERED: usize = 2;

#[derive(Default)]
pub struct Covers {
    windows: Vec<*mut Object>,
}

impl Covers {
    /// The displays, as they are.
    pub fn screens(&mut self, displays: Vec<Screen>, _: f32) -> Vec<Screen> {
        displays
    }

    /// The display with the mouse, where on the desktop it is, and whether
    /// a button is down.
    pub fn mouse(&mut self, _: &[Screen], _: f32) -> Option<Mouse> {
        // SAFETY: AppKit on the main thread; NSEvent's class methods give
        // plain values.
        unsafe {
            let main: BOOL = msg_send![class!(NSThread), isMainThread];
            if main != YES {
                return None;
            }
            let at: Point = msg_send![class!(NSEvent), mouseLocation];
            let buttons: usize = msg_send![class!(NSEvent), pressedMouseButtons];
            let display = screens().into_iter().find_map(|(id, frame)| {
                let inside = (frame.x..frame.x + frame.width).contains(&at.x)
                    && (frame.y..frame.y + frame.height).contains(&at.y);
                inside.then_some(id)
            })?;
            Some(Mouse {
                spot: Spot { display, at: None },
                at: (at.x, at.y),
                pressed: buttons != 0,
            })
        }
    }

    /// Cover `covers`, and nothing else.
    pub fn show(&mut self, covers: &[Screen], _: f32) {
        // SAFETY: as in `mouse`; each window is made here, released here,
        // and not released by AppKit as it closes.
        unsafe {
            let main: BOOL = msg_send![class!(NSThread), isMainThread];
            if main != YES {
                return;
            }
            for window in self.windows.drain(..) {
                let _: () = msg_send![window, orderOut: std::ptr::null_mut::<Object>()];
                let _: () = msg_send![window, close];
                let _: () = msg_send![window, release];
            }
            let screens = screens();
            for cover in covers {
                let Some(&(_, frame)) = screens.iter().find(|(id, _)| *id == cover.display) else {
                    continue;
                };
                let window: *mut Object = msg_send![class!(NSWindow), alloc];
                let window: *mut Object = msg_send![window,
                    initWithContentRect: frame
                    styleMask: BORDERLESS
                    backing: BUFFERED
                    defer: NO];
                if window.is_null() {
                    continue;
                }
                let black: *mut Object = msg_send![class!(NSColor), blackColor];
                let _: () = msg_send![window, setReleasedWhenClosed: NO];
                let _: () = msg_send![window, setBackgroundColor: black];
                let _: () = msg_send![window, setOpaque: YES];
                let _: () = msg_send![window, setHasShadow: NO];
                let _: () = msg_send![window, setLevel: LEVEL];
                let _: () = msg_send![window, setCollectionBehavior: BEHAVIOR];
                let _: () = msg_send![window, setFrame: frame display: YES];
                let _: () = msg_send![window, orderFrontRegardless];
                self.windows.push(window);
            }
        }
    }
}

/// Each screen's `CGDirectDisplayID` and frame, in AppKit's global terms.
unsafe fn screens() -> Vec<(u32, Rect)> {
    // SAFETY: as in `Covers::mouse`.
    unsafe {
        let key: *mut Object =
            msg_send![class!(NSString), stringWithUTF8String: c"NSScreenNumber".as_ptr()];
        let screens: *mut Object = msg_send![class!(NSScreen), screens];
        let count: usize = msg_send![screens, count];
        (0..count)
            .filter_map(|index| {
                let screen: *mut Object = msg_send![screens, objectAtIndex: index];
                let description: *mut Object = msg_send![screen, deviceDescription];
                let number: *mut Object = msg_send![description, objectForKey: key];
                if number.is_null() {
                    return None;
                }
                let id: u32 = msg_send![number, unsignedIntValue];
                let frame: Rect = msg_send![screen, frame];
                Some((id, frame))
            })
            .collect()
    }
}
