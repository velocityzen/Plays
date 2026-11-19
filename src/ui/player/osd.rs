//! Notes over a tile on what a shortcut did: a line of text, or the reel of
//! tracks rolling to the one picked. They show for a moment, then fade out.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, SharedString, Window, div};

use super::PlayerView;
use super::tracks::{TrackChoice, track_row};
use crate::ui::components::{ComponentSize, Panel};
use crate::ui::theme::motion::{OSD_DURATION, OSD_FADE, REEL_ROLL};
use crate::ui::theme::player::{NOTE_TOP, REEL_FADED, TRACK_MENU_WIDTH};
use crate::ui::theme::{menu, panel};

/// The note over a tile on what a shortcut did.
pub(super) enum Osd {
    Text(SharedString),
    /// A track picked with a shortcut.
    Reel(TrackReel),
}

/// The tracks of a kind on a reel, like a slot machine's: the one picked
/// in the middle, the ones before and after it faded above and below.
pub(super) struct TrackReel {
    /// In the order shortcuts go through them.
    pub(super) choices: Vec<TrackChoice>,
    /// The one picked.
    pub(super) at: usize,
}

impl PlayerView {
    /// Show a note on what a shortcut did.
    pub fn note(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.osd = Some((Osd::Text(text.into()), Instant::now()));
        cx.notify();
    }

    /// The note showing, below the header and fading as it ends; none once
    /// it is over.
    pub(super) fn render_osd(&mut self, window: &mut Window) -> Option<Div> {
        let osd = self.osd.as_ref().and_then(|(osd, shown)| {
            let shown = shown.elapsed();
            let left = OSD_DURATION.checked_sub(shown)?;
            let fade = (left.as_secs_f32() / OSD_FADE.as_secs_f32()).min(1.0);
            let note = match osd {
                Osd::Text(text) => Panel::note(text.clone()).into_any_element(),
                Osd::Reel(reel) => render_reel(reel, shown),
            };
            Some((note, fade))
        });
        if osd.is_some() {
            window.request_animation_frame();
        } else {
            self.osd = None;
        }
        osd.map(|(note, fade)| {
            div()
                .absolute()
                .top(NOTE_TOP)
                .left_0()
                .w_full()
                .flex()
                .justify_center()
                .opacity(fade)
                .child(note)
        })
    }
}

/// The reel `shown` after a track was picked: it rolls up a row, from the
/// one before in the middle to the one picked, which then has the one
/// before above it and the next below, faded.
fn render_reel(reel: &TrackReel, shown: Duration) -> AnyElement {
    let count = reel.choices.len();
    let row_height = ComponentSize::Medium.pick(menu::ROW_HEIGHT);
    let t = (shown.as_secs_f32() / REEL_ROLL.as_secs_f32()).min(1.0);
    let rolled = 1.0 - (1.0 - t).powi(3);
    // Rows from the middle once rolled: the one two before is in view above
    // until it rolls out.
    let rows: &[isize] = if count > 1 { &[-2, -1, 0, 1] } else { &[0] };
    let rows = rows.iter().filter_map(|&row| {
        let distance = row as f32 + if count > 1 { 1.0 - rolled } else { 0.0 };
        let opacity = reel_opacity(distance, count);
        if opacity <= 0.0 {
            return None;
        }
        let at = (reel.at as isize + row).rem_euclid(count as isize) as usize;
        Some(
            track_row(("reel", (row + 2) as usize), &reel.choices[at], row == 0)
                .overlay()
                .absolute()
                .left_0()
                .right_0()
                .top(row_height * (1.0 + distance))
                .opacity(opacity),
        )
    });
    Panel::new()
        .overlay()
        .w(TRACK_MENU_WIDTH)
        .p(panel::REEL_PADDING)
        .child(
            div()
                .relative()
                .h(row_height * 3.)
                .overflow_hidden()
                .children(rows),
        )
        .into_any_element()
}

/// How bright a row on the reel is `distance` rows from the middle (below
/// is positive): full there, faded a row away, gone half a row further.
/// With two tracks the one above is the one below, so only that shows.
fn reel_opacity(distance: f32, count: usize) -> f32 {
    let neighbor = match count {
        0 | 1 => 0.0,
        2 if distance < 0.0 => 0.0,
        _ => REEL_FADED,
    };
    let away = distance.abs();
    if away <= 1.0 {
        1.0 + (neighbor - 1.0) * away
    } else {
        neighbor * (1.0 - (away - 1.0) * 2.0).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{REEL_FADED, reel_opacity};

    #[test]
    fn fades_the_reel_away_from_the_middle() {
        let opacity = |distance, count| (reel_opacity(distance, count) * 100.0).round() / 100.0;
        assert_eq!(opacity(0.0, 5), 1.0);
        assert_eq!(opacity(-1.0, 5), REEL_FADED);
        assert_eq!(opacity(1.0, 5), REEL_FADED);
        assert_eq!(opacity(0.5, 5), 0.7);
        assert_eq!(opacity(1.5, 5), 0.0);
        assert_eq!(opacity(-2.0, 5), 0.0);
        // Two tracks: the other one shows below only.
        assert_eq!(opacity(1.0, 2), REEL_FADED);
        assert_eq!(opacity(-1.0, 2), 0.0);
        assert_eq!(opacity(0.0, 1), 1.0);
        assert_eq!(opacity(1.0, 1), 0.0);
    }
}
