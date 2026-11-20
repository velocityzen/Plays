//! The covers on X11: a black window over each monitor, which the window
//! manager leaves alone (override-redirect), so it is never framed or
//! focused. On a connection of its own, made the first time there is
//! something to cover; none on Wayland, where gpui runs then and windows
//! can't be placed.

use gpui::{point, px};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ConfigureWindowAux, ConnectionExt as _, CreateWindowAux, StackMode, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;

use super::{Mouse, Screen, Spot};
use crate::ui::displays;

/// Buttons 1 to 5 down, in a pointer's state.
const BUTTONS: u16 = 0x1f00;

#[derive(Default)]
pub struct Covers {
    x: Option<X>,
    tried: bool,
    windows: Vec<Window>,
}

struct X {
    connection: RustConnection,
    root: Window,
    black: u32,
}

impl Covers {
    fn x(&mut self) -> Option<&X> {
        if !self.tried {
            self.tried = true;
            self.x = connect();
        }
        self.x.as_ref()
    }

    /// Where the mouse is, and whether a button is down.
    pub fn mouse(&mut self, screens: &[Screen], scale: f32) -> Option<Mouse> {
        let x = self.x()?;
        let pointer = x.connection.query_pointer(x.root).ok()?.reply().ok()?;
        let (left, top) = (f32::from(pointer.root_x), f32::from(pointer.root_y));
        let at = point(px(left / scale), px(top / scale));
        // The monitor it is on, each a display.
        let display = screens
            .iter()
            .find(|screen| screen.has_point(at))
            .or(screens.first())?
            .display;
        Some(Mouse {
            spot: Spot {
                display,
                at: Some(at),
            },
            at: (left.into(), top.into()),
            pressed: u16::from(pointer.mask) & BUTTONS != 0,
        })
    }

    /// Cover `covers`, and nothing else.
    pub fn show(&mut self, covers: &[Screen], scale: f32) {
        self.x();
        let Some(x) = &self.x else {
            return;
        };
        for window in self.windows.drain(..) {
            x.connection.destroy_window(window).ok();
        }
        for cover in covers {
            let bounds = cover.bounds.map(|value| value * scale);
            let Ok(window) = x.connection.generate_id() else {
                continue;
            };
            let made = x.connection.create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                x.root,
                f32::from(bounds.origin.x) as i16,
                f32::from(bounds.origin.y) as i16,
                f32::from(bounds.size.width) as u16,
                f32::from(bounds.size.height) as u16,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new()
                    .background_pixel(x.black)
                    .override_redirect(1),
            );
            if made.is_err() {
                continue;
            }
            x.connection.map_window(window).ok();
            x.connection
                .configure_window(
                    window,
                    &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
                )
                .ok();
            self.windows.push(window);
        }
        x.connection.flush().ok();
    }
}

fn connect() -> Option<X> {
    if displays::placed_by_compositor() {
        return None;
    }
    let (connection, screen) = x11rb::connect(None).ok()?;
    let root = connection.setup().roots.get(screen)?;
    let (root, black) = (root.root, root.black_pixel);
    Some(X {
        connection,
        root,
        black,
    })
}
