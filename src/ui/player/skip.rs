//! Skipping the intro or credits playing, as a file's chapters mark them:
//! a button over the picture while they play, and a command. Past credits
//! a file ends with, the next one plays.

use gpui::prelude::*;
use gpui::{Context, Div, MouseButton, div};

use super::PlayerView;
use crate::media::chapters::{self, Part};
use crate::ui::components::Button;
use crate::ui::theme::player::CONTROLS_INSET;

/// How near its end skipping goes for it to count as going to the end:
/// past credits that run to it, the next file plays instead.
const AT_END: f64 = 0.5;

/// What skipping does now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Skip {
    /// Plays on from this time, past the intro or credits.
    Past(Part, f64),
    /// Plays the next file, past the credits this one ends with.
    NextFile,
}

impl Skip {
    /// What its button says.
    pub fn label(self) -> &'static str {
        match self {
            Skip::Past(Part::Intro, _) => "Skip Intro",
            Skip::Past(Part::Credits, _) => "Skip Credits",
            Skip::NextFile => "Next Episode",
        }
    }
}

impl PlayerView {
    /// What skipping does now, if anything: past the intro or credits
    /// playing or, past credits that run to its end, on to the next file
    /// in its playlist, or else its folder, where there's one.
    pub fn skip_now(&self) -> Option<Skip> {
        let (part, to) = chapters::skip(&self.chapters(), self.position())?;
        let ends = self
            .duration()
            .is_some_and(|duration| to > duration - AT_END);
        match (ends, part) {
            (false, _) => Some(Skip::Past(part, to)),
            (true, Part::Credits) if self.has_beside(1) => Some(Skip::NextFile),
            (true, _) => None,
        }
    }

    /// Skip as `skip_now` says, or say there's nothing to.
    pub fn skip(&mut self, cx: &mut Context<Self>) {
        match self.skip_now() {
            Some(Skip::Past(_, to)) => {
                self.seek(to);
                cx.notify();
            }
            Some(Skip::NextFile) => self.open_neighbor(1, cx),
            None => self.note("Nothing to skip", cx),
        }
    }

    /// The button that skips, low on the right above the controls, while
    /// there's something to skip. Presses on it don't reach the picture
    /// under it.
    pub(super) fn render_skip(&self, cx: &mut Context<Self>) -> Option<Div> {
        let skip = self.skip_now()?;
        let button = Button::new("skip", skip.label())
            .overlay()
            .rounded_full()
            .on_click(cx.listener(|this, _, _, cx| this.skip(cx)));
        Some(
            div()
                .debug_selector(|| "skip".into())
                .absolute()
                .right(CONTROLS_INSET)
                .bottom(self.subtitle_clearance() + CONTROLS_INSET)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(button),
        )
    }
}

#[cfg(test)]
#[path = "tests/skip.rs"]
mod tests;
