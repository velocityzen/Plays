//! How fast a block plays: faster, slower, or at its own speed again, the
//! sound keeping its pitch.

use gpui::Context;

use super::PlayerView;
use crate::app::refresh_menus;
use crate::media::tempo::{MAX_SPEED, MIN_SPEED};

/// The speeds Faster and Slower go through, and the Speed menu lists.
pub const SPEEDS: [f64; 10] = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0];

impl PlayerView {
    /// How fast it plays: 1 at its own speed.
    pub fn speed(&self) -> f64 {
        self.playback.local().speed()
    }

    /// Play at the next speed up (`faster`) or down from this one.
    pub fn change_speed(&mut self, faster: bool, cx: &mut Context<Self>) {
        self.set_speed(next_speed(self.speed(), faster), cx);
    }

    /// Play at `speed`, from `MIN_SPEED` to `MAX_SPEED`; not while it
    /// casts, as a device plays at its own.
    pub fn set_speed(&mut self, speed: f64, cx: &mut Context<Self>) {
        if let Some(device) = self.casting() {
            let note = format!("{} plays at its own speed", device.name);
            self.note(note, cx);
            return;
        }
        self.playback.set_speed(speed);
        self.note_change(format!("Speed {}", speed_text(self.speed())), cx);
        refresh_menus(cx);
    }
}

/// The speed after `speed` in `SPEEDS`, `faster` or slower; the fastest
/// or slowest there is past them.
pub fn next_speed(speed: f64, faster: bool) -> f64 {
    const NEAR: f64 = 1e-6;
    if faster {
        SPEEDS
            .into_iter()
            .find(|&each| each > speed + NEAR)
            .unwrap_or(MAX_SPEED)
    } else {
        SPEEDS
            .into_iter()
            .rev()
            .find(|&each| each < speed - NEAR)
            .unwrap_or(MIN_SPEED)
    }
}

/// A speed as notes and menus say it: `1×`, `1.5×`, `0.25×`.
pub fn speed_text(speed: f64) -> String {
    let rounded = (speed * 100.0).round() / 100.0;
    format!("{rounded}×")
}

#[cfg(test)]
#[path = "tests/speed.rs"]
mod tests;
