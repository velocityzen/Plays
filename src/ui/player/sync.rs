//! What the sync shortcuts and menu items do to a block: moving its sound
//! and its subtitles earlier or later against the picture, for files that
//! are out of step.

use gpui::Context;

use super::PlayerView;

impl PlayerView {
    /// Hear the sound `step` seconds later (negative: earlier).
    pub fn shift_audio(&mut self, step: f64, cx: &mut Context<Self>) {
        self.set_audio_delay(self.playback.local().audio_delay() + step, cx);
    }

    /// Hear the sound `delay` seconds after the picture (negative: before).
    pub fn set_audio_delay(&mut self, delay: f64, cx: &mut Context<Self>) {
        if !self.playback.local().has_audio() {
            self.note("No audio", cx);
            return;
        }
        self.playback.set_audio_delay(to_milliseconds(delay));
        self.send_cast_changes(cx);
        let note = format!(
            "Audio delay {}",
            delay_text(self.playback.local().audio_delay())
        );
        self.note_change(note, cx);
    }

    /// Show subtitles `step` seconds later (negative: earlier).
    pub fn shift_subtitles(&mut self, step: f64, cx: &mut Context<Self>) {
        self.set_subtitle_delay(self.playback.local().subtitle_delay() + step, cx);
    }

    /// Show subtitles `delay` seconds after their times (negative: before).
    pub fn set_subtitle_delay(&mut self, delay: f64, cx: &mut Context<Self>) {
        if !self.has_subtitles() {
            self.note("No subtitles", cx);
            return;
        }
        self.playback.set_subtitle_delay(to_milliseconds(delay));
        self.send_cast_changes(cx);
        let note = format!(
            "Subtitle delay {}",
            delay_text(self.playback.local().subtitle_delay())
        );
        self.note_change(note, cx);
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
#[path = "tests/sync.rs"]
mod tests;
