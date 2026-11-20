//! A tile's playback: the player here, and the device it plays on instead
//! while it casts. Every command the tile sends comes through here and
//! goes to the device or to the player here, as casting says, so no part
//! of the tile has to remember to ask first. Only this file can command
//! the player here; the rest of the tile reads it (`Playback::local`).
//! Where the player here is commanded while the tile casts, it is on
//! purpose, and its method says so: the picture following sound heard
//! elsewhere, or the player taking over again once a cast ends.

use std::sync::Arc;
use std::thread::JoinHandle;

use gpui::App;
use stream_server::Pick;

use super::PlayerView;
use super::cast::{Cast, NextCast, pick, subtitle_choice};
use crate::cast::Device;
use crate::media::player::Tick;
use crate::media::tracks::TrackKind;
use crate::media::{Mixer, Picture, Player, PlayerOptions, Source};
use crate::ui::components::Slide;
use crate::ui::theme::motion::END_FOLLOW_SEEK;

/// A tile's playback, here or on the device it casts to.
pub(super) struct Playback {
    /// The player here. Its commands take `&mut`, which only this file
    /// can get; the rest of the tile reads it through `local`.
    player: Player,
    /// The device it plays on instead, while it casts.
    pub(super) cast: Option<Cast>,
    /// The device it goes on casting to once the file just opened is
    /// known.
    pub(super) next_cast: Option<NextCast>,
    /// It played as the seek slider was pressed, so it plays on once the
    /// slider is let go.
    resume_after_scrub: bool,
}

impl Playback {
    pub fn open(source: Source, mixer: Option<Arc<Mixer>>, options: PlayerOptions) -> Self {
        Self {
            player: Player::open(source, mixer, options),
            cast: None,
            next_cast: None,
            resume_after_scrub: false,
        }
    }

    /// Open `source` here in place of what played, into the same mixer.
    /// A device it cast to lets go; the tile goes on casting the new file
    /// once it's known (`PlayerView::cast_once_known`).
    pub fn reopen(&mut self, source: Source, options: PlayerOptions) {
        self.cast = None;
        self.next_cast = None;
        self.player = Player::open(source, self.player.mixer(), options);
    }

    /// The player here, to read: what plays, what is in it, the picture,
    /// how playback goes. Its commands need `&mut`, so they can't go
    /// through this.
    pub fn local(&self) -> &Player {
        &self.player
    }

    /// The device the tile casts to, or goes on casting to with the file
    /// just opened.
    pub fn casting(&self) -> Option<&Device> {
        match &self.cast {
            Some(cast) => Some(cast.link.device()),
            None => self.next_cast.as_ref().map(|next| &next.device),
        }
    }

    /// It casts its sound only, the picture playing here.
    pub fn casts_sound_only(&self) -> bool {
        self.cast.as_ref().is_some_and(|cast| cast.picture_here)
    }

    /// It casts, picture and all: the tile shows where it plays instead.
    pub fn casts_picture(&self) -> bool {
        self.cast.as_ref().is_some_and(|cast| !cast.picture_here)
    }

    /// Whether it plays, here or on the device it casts to.
    pub fn is_playing(&self) -> bool {
        match &self.cast {
            Some(cast) => cast.link.is_playing(),
            None => self.player.is_playing(),
        }
    }

    /// Where it is, here or on the device it casts to.
    pub fn position(&self) -> f64 {
        match &self.cast {
            Some(cast) => cast.link.position(),
            None => self.player.position(),
        }
    }

    pub fn duration(&self) -> Option<f64> {
        match &self.cast {
            Some(cast) => cast.link.duration().or_else(|| self.player.duration()),
            None => self.player.duration(),
        }
    }

    /// Played to the end, here or there.
    pub fn has_ended(&self) -> bool {
        match &self.cast {
            Some(cast) => cast.link.has_ended(),
            None => self
                .duration()
                .is_some_and(|duration| self.position() >= duration - 0.05),
        }
    }

    pub fn play(&mut self) {
        match &mut self.cast {
            Some(cast) => cast.link.play(),
            None => self.player.play(),
        }
    }

    pub fn pause(&mut self) {
        match &mut self.cast {
            Some(cast) => {
                cast.link.pause();
                // A picture following the sound stops with it.
                if cast.picture_here {
                    self.player.pause();
                }
            }
            None => self.player.pause(),
        }
    }

    /// Seek to exactly `time`, here or there: a picture following the
    /// sound goes there too, and waits for the device.
    pub fn seek(&mut self, time: f64) {
        match &mut self.cast {
            Some(cast) => {
                cast.link.seek(time);
                if cast.picture_here {
                    self.player.seek(time);
                }
            }
            None => self.player.seek(time),
        }
    }

    /// Skip `seconds` forward or back. Here it lands on the nearest
    /// keyframe its way (`Player::seek_by`); a device, or the file just
    /// opened that is going to one, seeks exactly.
    pub fn seek_by(&mut self, seconds: f64) {
        if self.casting().is_some() {
            self.seek(self.position() + seconds);
        } else {
            self.player.seek_by(seconds);
        }
    }

    /// Show `time` as the seek slider is dragged, and seek there once it
    /// is let go. Here playback holds while it's dragged, and plays on
    /// after if it played. A device seeks only once the slider lets go:
    /// it is too far away to follow it. A picture here, following its
    /// sound, shows the way.
    pub fn scrub(&mut self, slide: Slide, time: f64) {
        if self.casting().is_some() {
            if slide == Slide::End {
                self.seek(time);
            } else if self.casts_sound_only() {
                self.player.scrub(time);
            }
            return;
        }
        match slide {
            Slide::Start => {
                self.resume_after_scrub = self.player.is_playing();
                self.player.pause();
                self.player.scrub(time);
            }
            Slide::Move => self.player.scrub(time),
            Slide::End => {
                self.player.seek(time);
                if self.resume_after_scrub {
                    self.player.play();
                }
            }
        }
    }

    /// Pause and show the next frame (`1`) or the one before (`-1`).
    /// Only here: a device can't step through frames, so while the tile
    /// casts nothing happens. Whether it stepped.
    pub fn step_frame(&mut self, direction: f64) -> bool {
        if self.casting().is_some() {
            return false;
        }
        let frame = 1.0
            / self
                .player
                .info()
                .and_then(|info| info.frame_rate)
                .filter(|fps| *fps > 0.0)
                .unwrap_or(25.0);
        let Some(pts) = self.player.frame_pts() else {
            return false;
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
        true
    }

    /// The tile's volume, which a cast takes along.
    pub fn set_volume(&mut self, volume: f32) {
        self.player.set_volume(volume);
        self.volume_changed();
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.player.set_muted(muted);
        self.volume_changed();
    }

    fn volume_changed(&mut self) {
        let (volume, muted) = (self.player.volume(), self.player.is_muted());
        if let Some(cast) = &mut self.cast {
            cast.link.set_volume(volume, muted);
        }
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.player.set_looping(looping);
        if let Some(cast) = &mut self.cast {
            cast.link.set_looping(looping);
        }
    }

    /// Play another track, or turn subtitles off (`None`), here and on the
    /// device: subtitles change there as they are, and other tracks load
    /// the media again from where it is.
    pub fn select_track(&mut self, kind: TrackKind, index: Option<usize>, cx: &App) {
        self.player.select_track(kind, index);
        let Some(cast) = &mut self.cast else {
            return;
        };
        // The track asked for, as the decoder may yet say an earlier one.
        let mut info = self.player.info();
        if let Some(info) = &mut info {
            Arc::make_mut(info).select(kind, index);
        }
        let subtitles = info.as_ref().and_then(|info| subtitle_choice(info, cx));
        // With the picture here, so are its subtitles: only the sound is
        // the device's.
        match (kind, cast.picture_here) {
            (TrackKind::Audio, true) => cast
                .link
                .reload(Pick::Off, pick(info.as_deref(), TrackKind::Audio)),
            (_, true) => {}
            (TrackKind::Subtitle, false) => cast.link.set_subtitles(subtitles),
            (TrackKind::Video | TrackKind::Audio, false) => cast.link.reload(
                pick(info.as_deref(), TrackKind::Video),
                pick(info.as_deref(), TrackKind::Audio),
            ),
        }
    }

    /// Hear the sound `delay` seconds after the picture (negative: before
    /// it). A device plays both, and a delay here doesn't reach it, so
    /// while the tile casts it stays as it is.
    pub fn set_audio_delay(&mut self, delay: f64) {
        if self.casting().is_none() {
            self.player.set_audio_delay(delay);
        }
    }

    /// Show subtitles `delay` seconds after their times (negative:
    /// before). As with the sound's, not while the tile casts.
    pub fn set_subtitle_delay(&mut self, delay: f64) {
        if self.casting().is_none() {
            self.player.set_subtitle_delay(delay);
        }
    }

    /// Move the player here along: frame queues, end of file, looping. It
    /// does while the tile casts too, as a picture following sound heard
    /// elsewhere plays here.
    pub fn tick(&mut self) -> Tick {
        self.player.tick()
    }

    /// Decode the picture here again if it is paused on a converted image,
    /// as pictures now look different (a new gamma). A device draws its
    /// own.
    pub fn refresh_picture(&mut self) {
        if !self.player.is_playing() && matches!(self.player.frame(), Some(Picture::Image(_))) {
            self.player.refresh();
        }
    }

    /// Play the sound here into `mixer` from now on.
    pub fn set_mixer(&mut self, mixer: Option<Arc<Mixer>>) {
        self.player.set_mixer(mixer);
    }

    /// Take every picture the player here may have drawn, so they can be
    /// released.
    pub fn take_pictures(&mut self) -> Vec<Picture> {
        self.player.take_pictures()
    }

    /// Play on the device `cast` links to from now on. The player here
    /// waits, paused; or, with only the sound going there, it plays on
    /// without it, its picture following the device (`follow_sound`).
    pub fn hand_over(&mut self, cast: Cast) {
        // A device it cast to before lets go, and with it the sound.
        self.let_go_of_cast();
        if cast.picture_here {
            self.player.set_sound_elsewhere(true);
        } else {
            self.player.pause();
        }
        self.cast = Some(cast);
    }

    /// Let go of the device it casts to, if any, without playing here from
    /// where it was: the sound is heard here again until another device
    /// plays it.
    pub fn let_go_of_cast(&mut self) {
        if self.cast.take().is_some_and(|cast| cast.picture_here) {
            self.player.set_sound_elsewhere(false);
        }
    }

    /// With the sound on the device, keep the picture here with it:
    /// playing once the device is seen to, and where it is. This commands
    /// the player here while the tile casts, as the picture is here.
    pub fn follow_sound(&mut self) {
        let Some(cast) = self.cast.as_ref().filter(|cast| cast.picture_here) else {
            return;
        };
        let (time, moving) = (cast.link.position(), cast.link.is_moving());
        if moving && !self.player.is_playing() {
            self.player.play();
        } else if !moving && self.player.is_playing() {
            self.player.pause();
        }
        self.player.follow(time);
    }

    /// Play here again, from where the device was, playing if it was; or,
    /// with the file just opened not yet gone there, as it would have
    /// played there. Whether the tile was casting.
    pub fn end_cast(&mut self) -> bool {
        if let Some(next) = self.next_cast.take() {
            // The file just opened hadn't gone there yet; it plays here as
            // it would have there.
            if next.autoplay {
                self.player.play();
            }
            return true;
        }
        let Some(cast) = self.cast.take() else {
            return false;
        };
        let picture_here = cast.picture_here;
        let (position, playing) = (cast.link.position(), cast.link.is_playing());
        // The device stops, and its URL goes, on the link's thread.
        drop(cast);
        if picture_here {
            self.player.set_sound_elsewhere(false);
        }
        // A picture that followed the sound is there already.
        let there = picture_here && (self.player.position() - position).abs() < END_FOLLOW_SEEK;
        if !self.player.is_live() && !there {
            self.player.seek(position);
        }
        if playing {
            self.player.play();
        } else {
            self.player.pause();
        }
        true
    }

    /// Stop the device as the app quits, handing back the thread stopping
    /// it, to wait for. The player here is left where the device was, so
    /// that's where it resumes.
    pub fn stop_cast(&mut self) -> Option<JoinHandle<()>> {
        self.next_cast = None;
        let cast = self.cast.take()?;
        if !self.player.is_live() {
            self.player.seek(cast.link.position());
        }
        cast.link.stop()
    }
}

/// The tile's playback, as the workspace, the menus and the shortcuts
/// reach it: each goes through `Playback`.
impl PlayerView {
    /// The player here, to read.
    pub fn player(&self) -> &Player {
        self.playback.local()
    }

    /// The device the tile casts to, or goes on casting to with the file
    /// just opened.
    pub fn casting(&self) -> Option<&Device> {
        self.playback.casting()
    }

    /// It casts its sound only, the picture playing here.
    #[cfg(test)]
    pub fn casts_sound_only(&self) -> bool {
        self.playback.casts_sound_only()
    }

    /// It casts, picture and all: the tile shows where it plays instead.
    pub(super) fn casts_picture(&self) -> bool {
        self.playback.casts_picture()
    }

    /// Whether it plays, here or on the device it casts to.
    pub fn is_playing(&self) -> bool {
        self.playback.is_playing()
    }

    /// Where it is, here or on the device it casts to.
    pub fn position(&self) -> f64 {
        self.playback.position()
    }

    pub fn duration(&self) -> Option<f64> {
        self.playback.duration()
    }

    /// Played to the end, here or there.
    pub(super) fn has_ended(&self) -> bool {
        self.playback.has_ended()
    }

    pub fn play(&mut self) {
        self.playback.play();
    }

    pub fn pause(&mut self) {
        self.playback.pause();
    }

    /// Seek to exactly `time`, here or there.
    pub fn seek(&mut self, time: f64) {
        self.playback.seek(time);
    }

    /// The tile's volume, which a cast takes along.
    pub fn set_volume(&mut self, volume: f32) {
        self.playback.set_volume(volume);
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.playback.set_muted(muted);
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.playback.set_looping(looping);
    }

    /// Play another track, or turn subtitles off (`None`), here and on the
    /// device.
    pub fn select_track(&mut self, kind: TrackKind, index: Option<usize>, cx: &App) {
        self.playback.select_track(kind, index, cx);
    }
}
