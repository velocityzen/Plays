//! What the sync shortcuts and menu items do to a tile: moving its sound
//! and its subtitles earlier or later against the picture, for files that
//! are out of step.

use gpui::Context;

use super::PlayerView;
use crate::media::tracks::TrackKind;

impl PlayerView {
    /// Hear the sound `step` seconds later (negative: earlier).
    pub fn shift_audio(&mut self, step: f64, cx: &mut Context<Self>) {
        self.set_audio_delay(self.playback.local().audio_delay() + step, cx);
    }

    /// Hear the sound `delay` seconds after the picture (negative: before).
    pub fn set_audio_delay(&mut self, delay: f64, cx: &mut Context<Self>) {
        if self.no_delays_while_cast(cx) {
            return;
        }
        if !self.playback.local().has_audio() {
            self.note("No audio", cx);
            return;
        }
        self.playback.set_audio_delay(to_milliseconds(delay));
        let note = format!(
            "Audio delay {}",
            delay_text(self.playback.local().audio_delay())
        );
        self.note(note, cx);
    }

    /// Show subtitles `step` seconds later (negative: earlier).
    pub fn shift_subtitles(&mut self, step: f64, cx: &mut Context<Self>) {
        self.set_subtitle_delay(self.playback.local().subtitle_delay() + step, cx);
    }

    /// Show subtitles `delay` seconds after their times (negative: before).
    pub fn set_subtitle_delay(&mut self, delay: f64, cx: &mut Context<Self>) {
        if self.no_delays_while_cast(cx) {
            return;
        }
        let has_subtitles = self
            .playback
            .local()
            .info()
            .is_some_and(|info| info.tracks(TrackKind::Subtitle).next().is_some());
        if !has_subtitles {
            self.note("No subtitles", cx);
            return;
        }
        self.playback.set_subtitle_delay(to_milliseconds(delay));
        let note = format!(
            "Subtitle delay {}",
            delay_text(self.playback.local().subtitle_delay())
        );
        self.note(note, cx);
    }
}

impl PlayerView {
    /// A device plays both the picture and the sound, so delays here don't
    /// reach it: say so, while casting.
    fn no_delays_while_cast(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(device) = self.casting() else {
            return false;
        };
        let note = format!("Delays don't apply on {}", device.name);
        self.note(note, cx);
        true
    }
}

/// Rounded to whole milliseconds, so steps add up exactly.
fn to_milliseconds(seconds: f64) -> f64 {
    (seconds * 1000.0).round() / 1000.0
}

/// A delay as a note says it: `+200 ms`, `−1.25 s`, `0 ms`.
fn delay_text(seconds: f64) -> String {
    let milliseconds = (seconds * 1000.0).round() as i64;
    let sign = match milliseconds.signum() {
        1 => "+",
        -1 => "−",
        _ => "",
    };
    let milliseconds = milliseconds.abs();
    if milliseconds < 1000 {
        return format!("{sign}{milliseconds} ms");
    }
    let seconds = format!("{:.3}", milliseconds as f64 / 1000.0);
    let seconds = seconds.trim_end_matches('0').trim_end_matches('.');
    format!("{sign}{seconds} s")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_delays_plainly() {
        assert_eq!(delay_text(0.0), "0 ms");
        assert_eq!(delay_text(-0.0), "0 ms");
        assert_eq!(delay_text(0.2), "+200 ms");
        assert_eq!(delay_text(-0.05), "−50 ms");
        assert_eq!(delay_text(1.0), "+1 s");
        assert_eq!(delay_text(-1.25), "−1.25 s");
        assert_eq!(delay_text(12.3), "+12.3 s");
        // Steps of 50 ms add up without drifting.
        let delay = (0..7).fold(0.0, |d, _| to_milliseconds(d + 0.05));
        assert_eq!(delay_text(delay), "+350 ms");
        assert_eq!(to_milliseconds(delay), 0.35);
    }
}
