//! What the picture shortcuts and menu items do to a block: turning and
//! flipping it, another aspect ratio, a crop, and back to as the file is.

use gpui::Context;

use super::PlayerView;
use crate::app::refresh_menus;
use crate::ui::framing::{ASPECT_RATIOS, Adjustments, CROPS, Ratio, next_ratio};

impl PlayerView {
    /// A quarter turn clockwise (`1`) or the other way (`-1`).
    pub fn rotate(&mut self, quarter_turns: i32, cx: &mut Context<Self>) {
        let note = if quarter_turns > 0 {
            "Rotated right"
        } else {
            "Rotated left"
        };
        self.adjust(note, cx, |a| {
            a.orientation = a.orientation.rotated(quarter_turns)
        });
    }

    /// Mirror the picture as it shows, left to right or upside down.
    pub fn flip(&mut self, horizontally: bool, cx: &mut Context<Self>) {
        if horizontally {
            self.adjust("Flipped horizontally", cx, |a| {
                a.orientation = a.orientation.flipped_horizontally();
            });
        } else {
            self.adjust("Flipped vertically", cx, |a| {
                a.orientation = a.orientation.flipped_vertically();
            });
        }
    }

    pub fn set_aspect_ratio(&mut self, aspect: Option<Ratio>, cx: &mut Context<Self>) {
        let note = match aspect {
            Some(aspect) => format!("Aspect ratio {}", aspect.name),
            None => "Aspect ratio as the file is".to_owned(),
        };
        self.adjust(note, cx, |a| a.aspect = aspect);
    }

    pub fn next_aspect_ratio(&mut self, cx: &mut Context<Self>) {
        self.set_aspect_ratio(next_ratio(self.adjustments.aspect, &ASPECT_RATIOS), cx);
    }

    pub fn set_crop(&mut self, crop: Option<Ratio>, cx: &mut Context<Self>) {
        let note = match crop {
            Some(crop) => format!("Crop {}", crop.name),
            None => "No crop".to_owned(),
        };
        self.adjust(note, cx, |a| a.crop = crop);
    }

    pub fn next_crop(&mut self, cx: &mut Context<Self>) {
        self.set_crop(next_ratio(self.adjustments.crop, &CROPS), cx);
    }

    /// Back to the picture as the file has it.
    pub fn reset_picture(&mut self, cx: &mut Context<Self>) {
        self.adjust("Picture reset", cx, |a| *a = Adjustments::default());
    }

    /// Change the adjustments and say how, if the block shows a video.
    fn adjust(
        &mut self,
        note: impl Into<gpui::SharedString>,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Adjustments),
    ) {
        if !self.playback.local().has_video() {
            self.note("No video", cx);
            return;
        }
        change(&mut self.adjustments);
        self.note_change(note, cx);
        // Video → Aspect Ratio and Crop check what it is now.
        refresh_menus(cx);
    }

    /// How it is turned, shaped and cropped.
    pub fn adjustments(&self) -> Adjustments {
        self.adjustments
    }
}
