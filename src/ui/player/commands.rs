//! What the playback shortcuts do to a block.

use gpui::{App, ClipboardItem, Context};

use super::PlayerView;
use super::controls::format_time;
use super::osd::TrackReel;
use super::tracks::track_choices;
use crate::app::refresh_menus;
use crate::media::chapters::{self, Chapter};
use crate::media::tracks::TrackKind;
use crate::ui::theme::motion::CHAPTER_RESTART;

impl PlayerView {
    pub fn play_pause(&mut self, cx: &mut Context<Self>) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
        cx.notify();
    }

    pub fn seek_by(&mut self, seconds: f64, cx: &mut Context<Self>) {
        self.playback.seek_by(seconds);
        cx.notify();
    }

    /// Play from `time`, in seconds, here or there (each stops at the
    /// end): where the seek bar was let go, say, which shows it until then.
    pub fn seek_to(&mut self, time: f64, cx: &mut Context<Self>) {
        self.scrub = None;
        self.playback.seek(time);
        cx.notify();
    }

    /// Copy the file's path, or the stream's URL as it was opened, its
    /// user and password too; what was copied.
    pub fn copy_path(&self, cx: &mut App) -> String {
        let address = self.playback.local().source().address();
        cx.write_to_clipboard(ClipboardItem::new_string(address.clone()));
        address
    }

    /// Copy where it is, as its bar says, such as 1:23:45; what was copied.
    pub fn copy_time(&self, cx: &mut App) -> String {
        let time = format_time(self.position());
        cx.write_to_clipboard(ClipboardItem::new_string(time.clone()));
        time
    }

    pub fn restart(&mut self, cx: &mut Context<Self>) {
        self.seek(0.0);
        cx.notify();
    }

    /// The file's chapters, once it's open.
    pub fn chapters(&self) -> Vec<Chapter> {
        self.playback
            .local()
            .info()
            .map(|info| info.chapters.clone())
            .unwrap_or_default()
    }

    /// The chapter it's in.
    pub fn chapter(&self) -> Option<usize> {
        chapters::at(&self.chapters(), self.position())
    }

    /// Go to the start of the next chapter.
    pub fn next_chapter(&mut self, cx: &mut Context<Self>) {
        let chapters = self.chapters();
        match chapters::next(&chapters, self.position()) {
            Some(index) => self.go_to_chapter(index, cx),
            None if chapters.is_empty() => self.note("No chapters", cx),
            None => self.note("The last chapter", cx),
        }
    }

    /// Back to the start of this chapter, or of the one before near its
    /// start.
    pub fn previous_chapter(&mut self, cx: &mut Context<Self>) {
        let chapters = self.chapters();
        match chapters::previous(&chapters, self.position(), CHAPTER_RESTART) {
            Some(index) => self.go_to_chapter(index, cx),
            None if chapters.is_empty() => self.note("No chapters", cx),
            None => {
                self.seek(0.0);
                cx.notify();
            }
        }
    }

    /// Play from the start of chapter `index`, saying which it is.
    pub fn go_to_chapter(&mut self, index: usize, cx: &mut Context<Self>) {
        let chapters = self.chapters();
        let Some(chapter) = chapters.get(index) else {
            return;
        };
        self.seek(chapter.start);
        self.note_change(chapter.name(index), cx);
        // Playback → Chapters checks this one now.
        refresh_menus(cx);
    }

    /// Pause and show the next frame (`1`) or the one before (`-1`).
    pub fn step_frame(&mut self, direction: f64, cx: &mut Context<Self>) {
        if let Some(device) = self.casting() {
            let note = format!("Can't step through frames on {}", device.name);
            self.note(note, cx);
            return;
        }
        if self.playback.step_frame(direction) {
            cx.notify();
        }
    }

    pub fn change_volume(&mut self, delta: f32, cx: &mut Context<Self>) {
        let volume = (self.playback.local().volume() + delta).clamp(0.0, 1.0);
        self.set_volume(volume);
        if delta > 0.0 && self.playback.local().is_muted() {
            self.set_muted(false);
        }
        self.note_change(format!("Volume {}%", (volume * 100.0).round()), cx);
    }

    /// Set the volume, from 0 to 1; above none, it's no longer muted.
    pub fn set_volume_to(&mut self, volume: f32, cx: &mut Context<Self>) {
        self.set_volume(volume);
        if volume > 0.0 && self.playback.local().is_muted() {
            self.set_muted(false);
        }
        cx.notify();
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        let muted = !self.playback.local().is_muted();
        self.set_muted(muted);
        self.note_change(if muted { "Muted" } else { "Unmuted" }, cx);
    }

    pub fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        let looping = !self.playback.local().is_looping();
        self.set_looping(looping);
        self.note_change(if looping { "Loop on" } else { "Loop off" }, cx);
    }

    /// Play the next track of a kind, after the last one going back to the
    /// first (and, for subtitles, to none), and roll the reel to it.
    pub fn next_track(&mut self, kind: TrackKind, cx: &mut Context<Self>) {
        let Some(info) = self.playback.local().info() else {
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
        self.select_track(kind, choices[at].index, cx);
        if kind == TrackKind::Subtitle {
            self.hidden_subtitles = None;
        }
        self.roll_reel(TrackReel { choices, at }, cx);
    }

    /// Play track `number` (from 1) of a kind, as the tracks menu numbers
    /// them, or no subtitles (`None`). Whether the file has that track.
    pub fn select_track_number(
        &mut self,
        kind: TrackKind,
        number: Option<usize>,
        cx: &mut Context<Self>,
    ) -> bool {
        let index = match number {
            None if kind == TrackKind::Subtitle => None,
            None => return false,
            Some(number) => {
                let info = self.playback.local().info();
                let track = info.as_ref().and_then(|info| {
                    let at = number.checked_sub(1)?;
                    info.tracks(kind).nth(at).map(|track| track.index)
                });
                match track {
                    Some(index) => Some(index),
                    None => return false,
                }
            }
        };
        self.select_track(kind, index, cx);
        cx.notify();
        true
    }

    /// Hide the subtitles showing, or show the ones hidden (or the first).
    pub fn toggle_subtitles(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.playback.local().info() else {
            return;
        };
        if let Some(index) = info.subtitle_track {
            self.hidden_subtitles = Some(index);
            self.select_track(TrackKind::Subtitle, None, cx);
            self.note_change("Subtitles hidden", cx);
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
                let index = tracks[i].index;
                self.select_track(TrackKind::Subtitle, Some(index), cx);
                self.note_change(format!("Subtitles: {}", tracks[i].label(i + 1)), cx);
            }
            None => self.note("No subtitles", cx),
        }
    }
}
