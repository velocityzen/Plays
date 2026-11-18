//! What the playback shortcuts do to a tile.

use std::time::Instant;

use gpui::Context;

use super::PlayerView;
use super::osd::{Osd, TrackReel};
use super::tracks::track_choices;
use crate::media::tracks::TrackKind;

impl PlayerView {
    pub fn play_pause(&mut self, cx: &mut Context<Self>) {
        self.player.toggle();
        cx.notify();
    }

    pub fn seek_by(&mut self, seconds: f64, cx: &mut Context<Self>) {
        self.player.seek_by(seconds);
        cx.notify();
    }

    pub fn restart(&mut self, cx: &mut Context<Self>) {
        self.player.seek(0.0);
        cx.notify();
    }

    /// Pause and show the next frame (`1`) or the one before (`-1`).
    pub fn step_frame(&mut self, direction: f64, cx: &mut Context<Self>) {
        let frame = 1.0
            / self
                .player
                .info()
                .and_then(|info| info.frame_rate)
                .filter(|fps| *fps > 0.0)
                .unwrap_or(25.0);
        let Some(pts) = self.player.frame_pts() else {
            return;
        };
        self.player.pause();
        // A seek shows the first frame that hasn't ended by its target, so
        // aim into the middle of the frame wanted.
        let target = if direction > 0.0 {
            pts + 1.5 * frame
        } else {
            pts - 0.5 * frame
        };
        self.player.seek(target.max(0.0));
        cx.notify();
    }

    pub fn change_volume(&mut self, delta: f32, cx: &mut Context<Self>) {
        let volume = (self.player.volume() + delta).clamp(0.0, 1.0);
        self.player.set_volume(volume);
        if delta > 0.0 && self.player.is_muted() {
            self.player.set_muted(false);
        }
        self.note(format!("Volume {}%", (volume * 100.0).round()), cx);
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        let muted = !self.player.is_muted();
        self.player.set_muted(muted);
        self.note(if muted { "Muted" } else { "Unmuted" }, cx);
    }

    pub fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        let looping = !self.player.is_looping();
        self.player.set_looping(looping);
        self.note(if looping { "Loop on" } else { "Loop off" }, cx);
    }

    /// Play the next track of a kind, after the last one going back to the
    /// first (and, for subtitles, to none), and roll the reel to it.
    pub fn next_track(&mut self, kind: TrackKind, cx: &mut Context<Self>) {
        let Some(info) = self.player.info() else {
            return;
        };
        let choices = track_choices(&info, kind);
        if choices.iter().all(|choice| choice.index.is_none()) {
            let note = match kind {
                TrackKind::Video => "No video tracks",
                TrackKind::Audio => "No audio tracks",
                TrackKind::Subtitle => "No subtitles",
            };
            self.note(note, cx);
            return;
        }
        let selected = info.selected(kind);
        let at = choices
            .iter()
            .position(|choice| choice.index == selected)
            .map_or(0, |at| (at + 1) % choices.len());
        self.player.select_track(kind, choices[at].index);
        if kind == TrackKind::Subtitle {
            self.hidden_subtitles = None;
        }
        let reel = TrackReel { choices, at };
        self.osd = Some((Osd::Reel(reel), Instant::now()));
        cx.notify();
    }

    /// Hide the subtitles showing, or show the ones hidden (or the first).
    pub fn toggle_subtitles(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.player.info() else {
            return;
        };
        if let Some(index) = info.subtitle_track {
            self.hidden_subtitles = Some(index);
            self.player.select_track(TrackKind::Subtitle, None);
            self.note("Subtitles hidden", cx);
            return;
        }
        let tracks: Vec<_> = info.tracks(TrackKind::Subtitle).collect();
        let shown = self
            .hidden_subtitles
            .take()
            .and_then(|index| tracks.iter().position(|t| t.index == index))
            .or((!tracks.is_empty()).then_some(0));
        match shown {
            Some(i) => {
                self.player
                    .select_track(TrackKind::Subtitle, Some(tracks[i].index));
                self.note(format!("Subtitles: {}", tracks[i].label(i + 1)), cx);
            }
            None => self.note("No subtitles", cx),
        }
    }
}
