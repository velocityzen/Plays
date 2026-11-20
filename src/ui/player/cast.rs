//! Casting a tile: playing its file on a TV or a speaker instead, from
//! where it is, with its tracks. While it casts, the tile's controls and
//! shortcuts drive the device and its bar follows the device; its own
//! playback waits, paused, and goes on from where the device was once the
//! cast ends. A video cast to a speaker sends only its sound, and the
//! picture plays on here, following the speaker.
//!
//! The cast button's menu also lists the displays, with more than one, to
//! put the window full screen on, as Video → Output Device does on macOS;
//! and, with more than one audio output, those the window can play its
//! sound through instead of the app's, as Audio → Window Output Device
//! does: the only place to pick them on Linux, which has no menu bar.

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Corner, Div, ElementId, MouseDownEvent, Task, Window, div};
use stream_server::{Pick, SubtitleSource};

use super::PlayerView;
use crate::app::{AudioEngine, SetWindowAudioDevice, ShowOnDisplay};
use crate::cast::target::FOLLOW_POLL;
use crate::cast::{Casting, Device, Error, Link, Phase, Request, Shows, SubtitleChoice};
use crate::media::Source;
use crate::media::decoder::MediaInfo;
use crate::media::output::DeviceInfo;
use crate::media::tracks::TrackKind;
use crate::settings::{AudioDevice, Settings};
use crate::ui::components::{Button, ClickHandler, Icon, MenuRow, Panel, Popover};
use crate::ui::displays::{self, DisplayInfo};
use crate::ui::framing::Adjustments;
use crate::ui::theme::Theme;
use crate::ui::theme::motion::{CAST_POLL, END_FOLLOW_SEEK, NEXT_CAST_POLL};
use crate::ui::theme::player::{CAST_MENU_WIDTH, ERROR_MAX_WIDTH, PIP_ICON, TRACK_MENU_OFFSET};
use crate::ui::theme::text::TextStyle;

/// A tile's cast.
pub(super) struct Cast {
    link: Link,
    /// It has played there, so its ending is the device going away rather
    /// than it not starting.
    on: bool,
    /// Only the sound goes; the picture plays here, following the device.
    picture_here: bool,
    /// Keeps the tile up to date with the device.
    _follow: Task<()>,
}

/// A cast going on with the file just opened, once it's known what's in
/// it: whether it has sound for a speaker, a picture to keep here, and
/// subtitles all decide what goes.
pub(super) struct NextCast {
    device: Device,
    autoplay: bool,
    _wait: Task<()>,
}

impl PlayerView {
    /// The device the tile casts to, or goes on casting to with the file
    /// just opened.
    pub fn casting(&self) -> Option<&Device> {
        match &self.cast {
            Some(cast) => Some(cast.link.device()),
            None => self.next_cast.as_ref().map(|next| &next.device),
        }
    }

    /// Go on casting to `device` once the file just opened is known, or
    /// has failed to open, from where it is then.
    pub(super) fn cast_once_known(
        &mut self,
        device: Device,
        autoplay: bool,
        cx: &mut Context<Self>,
    ) {
        let wait = cx.spawn(async move |this, cx| {
            loop {
                let known = this.update(cx, |this, cx| {
                    let player = &this.player;
                    if player.info().is_none() && player.error().is_none() {
                        return false;
                    }
                    if let Some(next) = this.next_cast.take() {
                        let start = this.player.position();
                        this.start_cast(next.device, start, next.autoplay, cx);
                    }
                    true
                });
                if !matches!(known, Ok(false)) {
                    break;
                }
                cx.background_executor().timer(NEXT_CAST_POLL).await;
            }
        });
        self.next_cast = Some(NextCast {
            device,
            autoplay,
            _wait: wait,
        });
    }

    /// It casts its sound only, the picture playing here.
    pub fn casts_sound_only(&self) -> bool {
        self.cast.as_ref().is_some_and(|cast| cast.picture_here)
    }

    /// It casts, picture and all: the tile shows where it plays instead.
    pub(super) fn casts_picture(&self) -> bool {
        self.cast.as_ref().is_some_and(|cast| !cast.picture_here)
    }

    /// Cast to `device` from where the tile is, playing if it plays; or
    /// with none, play here again from where the device was.
    pub fn cast_to(&mut self, device: Option<Device>, cx: &mut Context<Self>) {
        self.cast_open = false;
        match device {
            None => {
                self.end_cast(cx);
            }
            Some(device) if self.casting().is_some_and(|cast| cast.is(&device)) => {}
            Some(device) => {
                let (start, autoplay) = (self.position(), self.is_playing());
                self.start_cast(device, start, autoplay, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn start_cast(
        &mut self,
        device: Device,
        start: f64,
        autoplay: bool,
        cx: &mut Context<Self>,
    ) {
        // Another device picked meanwhile is the one it casts to.
        self.next_cast = None;
        if self.player.error().is_some() {
            self.note("Can't cast a file that doesn't play", cx);
            return;
        }
        let info = self.player.info();
        if device.shows == Shows::Sound && info.as_ref().is_some_and(|info| !info.has_audio) {
            self.note(
                format!("{} plays only sound, and this has none", device.name),
                cx,
            );
            return;
        }
        let picture_here =
            device.shows == Shows::Sound && info.as_ref().is_some_and(|info| info.has_video);
        let request = self.cast_request(info.as_ref(), start, autoplay, picture_here, cx);
        // Another device this casts to lets go; its sound is here again
        // until the new one plays it.
        if self.cast.take().is_some_and(|cast| cast.picture_here) {
            self.player.set_sound_elsewhere(false);
        }
        match Casting::link(cx, &device, request) {
            Ok(link) => {
                if picture_here {
                    self.player.set_sound_elsewhere(true);
                } else {
                    self.player.pause();
                    // The picture plays there now, not in a window here.
                    self.close_picture_in_picture();
                }
                // A picture following the sound looks as often as the
                // link asks where the device is.
                let every = if picture_here { FOLLOW_POLL } else { CAST_POLL };
                let follow = cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(every).await;
                        let going = this.update(cx, |this, cx| this.follow_cast(cx));
                        if !matches!(going, Ok(true)) {
                            break;
                        }
                    }
                });
                self.cast = Some(Cast {
                    link,
                    on: false,
                    picture_here,
                    _follow: follow,
                });
            }
            Err(why) => self.note(format!("Can't play on {}: {why}", device.name), cx),
        }
    }

    /// What to cast: the file or stream, with the tracks playing here; or,
    /// with the picture here, its sound.
    fn cast_request(
        &self,
        info: Option<&MediaInfo>,
        start: f64,
        autoplay: bool,
        picture_here: bool,
        cx: &App,
    ) -> Request {
        let source = match self.player.source() {
            Source::File(path) => stream_server::Source::File(path.clone()),
            Source::Url(url) => stream_server::Source::Stream {
                url: url.clone(),
                live: info.is_some_and(|info| info.live),
            },
        };
        Request {
            source,
            title: info
                .and_then(|info| info.title.clone())
                .unwrap_or_else(|| self.title.to_string()),
            video: if picture_here {
                Pick::Off
            } else {
                pick(info, TrackKind::Video)
            },
            audio: pick(info, TrackKind::Audio),
            subtitles: info
                .filter(|_| !picture_here)
                .and_then(|info| subtitle_choice(info, cx)),
            start,
            autoplay,
            volume: self.player.volume(),
            muted: self.player.is_muted(),
            looping: self.player.is_looping(),
            picture_here,
        }
    }

    /// Play here again, from where the device was, playing if it was.
    /// Whether the tile was casting.
    pub fn end_cast(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(next) = self.next_cast.take() {
            // The file just opened hadn't gone there yet; it plays here as
            // it would have there.
            if next.autoplay {
                self.player.play();
            }
            cx.notify();
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
        cx.notify();
        true
    }

    /// Stop the device as the app quits, handing back the thread stopping
    /// it, to wait for. The player here is left where the device was, so
    /// that's where it resumes.
    pub fn stop_cast(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.next_cast = None;
        let cast = self.cast.take()?;
        if !self.player.is_live() {
            self.player.seek(cast.link.position());
        }
        cast.link.stop()
    }

    /// Show what the device does, say what went wrong, and end the cast
    /// once it's over. Whether it goes on.
    fn follow_cast(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(cast) = &mut self.cast else {
            return false;
        };
        let name = cast.link.device().name.clone();
        if let Some(why) = cast.link.take_failure() {
            self.note(format!("{name}: {why}"), cx);
        }
        let Some(cast) = &mut self.cast else {
            return false;
        };
        match cast.link.phase() {
            Phase::Connecting => {}
            Phase::On { .. } if !cast.on => {
                cast.on = true;
                if cast.picture_here {
                    self.note(format!("Sound on {name}"), cx);
                }
            }
            Phase::On { .. } => {}
            Phase::Over(err) => {
                let note = match err {
                    Error::Gone if cast.on => format!("{name} went away"),
                    Error::Replaced if cast.on => format!("{name} plays something else now"),
                    Error::Stopped if cast.on => format!("{name} stopped playing it"),
                    err => format!("Can't play on {name}: {err}"),
                };
                self.end_cast(cx);
                self.note(note, cx);
                return false;
            }
        }
        self.follow_sound();
        cx.notify();
        true
    }

    /// With the sound on the device, keep the picture with it: playing
    /// once the device is seen to, and where it is. While the slider is
    /// dragged it shows where the slider is, the device seeking only once
    /// it's let go.
    fn follow_sound(&mut self) {
        let Some(cast) = self
            .cast
            .as_ref()
            .filter(|cast| cast.picture_here && self.scrub.is_none())
        else {
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
    pub(super) fn has_ended(&self) -> bool {
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
        if self.cast.is_none() {
            return;
        }
        // The track asked for, as the decoder may yet say an earlier one.
        let mut info = self.player.info();
        if let Some(info) = &mut info {
            info.select(kind, index);
        }
        let subtitles = info.as_ref().and_then(|info| subtitle_choice(info, cx));
        let Some(cast) = &mut self.cast else {
            return;
        };
        // With the picture here, so are its subtitles: only the sound is
        // the device's.
        match (kind, cast.picture_here) {
            (TrackKind::Audio, true) => cast
                .link
                .reload(Pick::Off, pick(info.as_ref(), TrackKind::Audio)),
            (_, true) => {}
            (TrackKind::Subtitle, false) => cast.link.set_subtitles(subtitles),
            (TrackKind::Video | TrackKind::Audio, false) => cast.link.reload(
                pick(info.as_ref(), TrackKind::Video),
                pick(info.as_ref(), TrackKind::Audio),
            ),
        }
    }

    /// What of the tile the device doesn't get, while it casts.
    #[cfg(test)]
    pub fn cast_stays_here(&self, cx: &App) -> Vec<&'static str> {
        self.cast
            .as_ref()
            .map_or_else(Vec::new, |cast| self.stays_here(&cast.link.phase(), cx))
    }

    fn stays_here(&self, phase: &Phase, cx: &App) -> Vec<&'static str> {
        let settings = cx.global::<Settings>();
        let adjusted = self.adjustments != Adjustments::default() || settings.gamma() != [1.0; 2];
        let drawn = matches!(
            phase,
            Phase::On {
                draws_subtitles: true,
                ..
            }
        );
        let picture_subtitles = !drawn
            && self.player.info().is_some_and(|info| {
                info.subtitle_track.is_some_and(|index| {
                    info.tracks(TrackKind::Subtitle)
                        .any(|track| track.index == index && !track.is_text())
                })
            });
        [
            adjusted
                .then_some("Gamma, crop, rotation, flips and aspect ratio stay on this computer"),
            picture_subtitles.then_some("Picture subtitles stay on this computer"),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// What fills the tile while it casts: where it plays, how the media
    /// gets there, what stays here, and a button to play it here again.
    pub(super) fn render_cast(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let cast = self.cast.as_ref().filter(|cast| !cast.picture_here)?;
        let device = cast.link.device();
        let phase = cast.link.phase();
        let (line, detail) = match &phase {
            Phase::On { serves, .. } => (
                format!("Playing on {}", device.name),
                Some(format!("{} · {serves}", device.kind)),
            ),
            _ => (format!("Connecting to {}…", device.name), None),
        };
        let stays = self.stays_here(&phase, cx);
        Some(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .max_w(ERROR_MAX_WIDTH)
                .px_4()
                .text_center()
                .child(Icon::new("cast").size(PIP_ICON).color(theme.text_muted))
                .child(div().text_sm().text_color(theme.text).child(line))
                .when_some(detail, |this, detail| {
                    this.child(div().text_xs().text_color(theme.text_muted).child(detail))
                })
                .children(
                    stays
                        .into_iter()
                        .map(|line| div().text_xs().text_color(theme.text_muted).child(line)),
                )
                .child(
                    Button::new("play-here", "Play Here")
                        .mt_1()
                        .on_click(cx.listener(|this, _, _, cx| this.cast_to(None, cx))),
                )
                .into_any_element(),
        )
    }

    /// The cast button, while there are devices to cast to, displays or
    /// audio outputs to pick, and the menu of them it opens.
    pub(super) fn render_cast_button(
        &self,
        info: Option<&MediaInfo>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        // Until the file says, it may well have sound.
        let has_audio = info.is_none_or(|info| info.has_audio);
        let has_video = info.is_some_and(|info| info.has_video);
        let mut devices = Casting::devices_for(cx, has_audio);
        if let Some(casting) = self.casting()
            && !devices.iter().any(|device| device.is(casting))
        {
            devices.push(casting.clone());
        }
        let mut screens = displays::connected(cx);
        if screens.len() < 2 {
            screens.clear();
        }
        let outputs = AudioEngine::devices(cx);
        let outputs = (outputs.len() > 1 || self.window_sound.is_some()).then(|| outputs.to_vec());
        if devices.is_empty() && screens.is_empty() && outputs.is_none() {
            return None;
        }
        let button = Button::icon("cast", "cast")
            .overlay()
            .selected(self.cast.is_some())
            .on_click(cx.listener(|this, _, _, cx| {
                this.cast_open = !this.cast_open;
                // As Linux doesn't say when outputs come and go.
                if this.cast_open {
                    AudioEngine::list_devices(cx);
                }
                cx.notify();
            }));
        Some(
            Popover::new("cast-backdrop", button)
                .anchor(Corner::BottomRight, TRACK_MENU_OFFSET)
                .when(self.cast_open, |this| {
                    let here = (!screens.is_empty())
                        .then(|| displays::of(window, cx))
                        .flatten();
                    this.menu(
                        self.render_cast_menu(
                            devices, screens, here, outputs, has_video, theme, cx,
                        ),
                    )
                })
                .on_dismiss(cx.listener(close_cast_menu))
                .into_any_element(),
        )
    }

    /// This computer, or each display (`screens`) with the window's
    /// (`here`) checked, and the devices: the one playing checked. A
    /// speaker says it takes only a video's sound. Picking a display puts
    /// the window full screen there (`ShowOnDisplay`), or plays the tile
    /// here again while it casts, if the window is on it; Wayland may keep
    /// it on its own display, which the menu says. Then the window's sound
    /// (`render_window_sound`), with `outputs` to pick from.
    #[allow(clippy::too_many_arguments)]
    fn render_cast_menu(
        &self,
        devices: Vec<Device>,
        screens: Vec<DisplayInfo>,
        here: Option<u32>,
        outputs: Option<Vec<DeviceInfo>>,
        has_video: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let play_on = !devices.is_empty() || !screens.is_empty();
        let sound = outputs.map(|outputs| self.render_window_sound(outputs, cx));
        let casting = self.casting().cloned();
        let this_computer = screens.is_empty().then(|| {
            MenuRow::new("cast-here", "This Computer")
                .checked(casting.is_none())
                .mx_1()
                .on_click(cx.listener(|this, _, _, cx| this.cast_to(None, cx)))
        });
        let wayland = !screens.is_empty() && displays::placed_by_compositor();
        let screens = screens.into_iter().enumerate().map(|(i, screen)| {
            MenuRow::new(
                ElementId::Name(format!("display-{i}").into()),
                screen.name.clone(),
            )
            .checked(casting.is_none() && here == Some(screen.id))
            .detail(div().text_xs().child("Full screen"))
            .mx_1()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.cast_open = false;
                window.dispatch_action(Box::new(ShowOnDisplay(screen.id)), cx);
                cx.notify();
            }))
        });
        let rows = devices.into_iter().enumerate().map(|(i, device)| {
            let checked = casting.as_ref().is_some_and(|casting| casting.is(&device));
            let detail = if has_video && device.shows == Shows::Sound {
                format!("{} · sound only", device.kind)
            } else {
                device.kind.to_owned()
            };
            MenuRow::new(
                ElementId::Name(format!("cast-{i}").into()),
                device.name.clone(),
            )
            .checked(checked)
            .detail(div().text_xs().child(detail))
            .mx_1()
            .on_click(cx.listener(move |this, _, _, cx| this.cast_to(Some(device.clone()), cx)))
        });
        Panel::new()
            .occlude()
            .flex()
            .flex_col()
            .w(CAST_MENU_WIDTH)
            .pb_1()
            .when(play_on, |this| {
                this.child(heading("Play On", theme))
                    .children(this_computer)
                    .children(screens)
                    .when(wayland, |this| {
                        this.child(
                            div()
                                .px_3()
                                .py_1()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child("Wayland may keep it where it is"),
                        )
                    })
                    .children(rows)
            })
            .when_some(sound, |this, sound| {
                this.child(heading("Window Sound", theme)).children(sound)
            })
            .into_any_element()
    }

    /// The app's output, checked unless the window plays through one of its
    /// own; then `outputs`, the window's own checked, and listed too while
    /// it isn't connected.
    /// Picking one plays the whole window through it
    /// (`SetWindowAudioDevice`).
    fn render_window_sound(
        &self,
        outputs: Vec<DeviceInfo>,
        cx: &mut Context<Self>,
    ) -> Vec<MenuRow> {
        let own = self.window_sound.clone();
        let mut rows = vec![
            MenuRow::new("sound-app", "Same as the App")
                .checked(own.is_none())
                .mx_1()
                .on_click(pick_window_sound(None, cx)),
        ];
        let missing = own
            .clone()
            .filter(|own| !outputs.iter().any(|output| output.id == own.id));
        for (i, output) in outputs.into_iter().enumerate() {
            let device = AudioDevice {
                id: output.id,
                name: output.name,
            };
            let checked = own.as_ref().is_some_and(|own| own.id == device.id);
            rows.push(
                MenuRow::new(
                    ElementId::Name(format!("sound-{i}").into()),
                    device.name.clone(),
                )
                .checked(checked)
                .mx_1()
                .on_click(pick_window_sound(Some(device), cx)),
            );
        }
        if let Some(device) = missing {
            rows.push(
                MenuRow::new("sound-missing", device.name.clone())
                    .checked(true)
                    .detail(div().text_xs().child("Not connected"))
                    .mx_1()
                    .on_click(pick_window_sound(Some(device), cx)),
            );
        }
        rows
    }
}

/// Clicks that play the window through `device`, or the app's output.
fn pick_window_sound(device: Option<AudioDevice>, cx: &mut Context<PlayerView>) -> ClickHandler {
    Box::new(cx.listener(move |view, _, window, cx| {
        view.cast_open = false;
        window.dispatch_action(Box::new(SetWindowAudioDevice(device.clone())), cx);
        cx.notify();
    }))
}

/// Which track of a kind to cast: the one playing here.
fn pick(info: Option<&MediaInfo>, kind: TrackKind) -> Pick {
    info.and_then(|info| info.selected(kind))
        .map_or(Pick::Best, Pick::Stream)
}

/// The subtitles showing, to send along: text ones, and picture ones in
/// the media, which the stream server draws into video it re-encodes.
/// Picture subtitles from a file of their own stay here.
fn subtitle_choice(info: &MediaInfo, cx: &App) -> Option<SubtitleChoice> {
    let index = info.subtitle_track?;
    let (number, track) = info
        .tracks(TrackKind::Subtitle)
        .enumerate()
        .find(|(_, track)| track.index == index)?;
    if !track.is_text() && track.file.is_some() {
        return None;
    }
    let source = match &track.file {
        Some(path) => SubtitleSource::File {
            path: path.clone(),
            encoding: None,
        },
        None => SubtitleSource::Track(index),
    };
    Some(SubtitleChoice {
        source,
        language: track.language.clone(),
        name: track.label(number + 1),
        style: cx.global::<Settings>().subtitles.clone(),
    })
}

/// Clicks outside the menu close it, and do nothing else.
fn close_cast_menu(
    view: &mut PlayerView,
    _: &MouseDownEvent,
    _: &mut Window,
    cx: &mut Context<PlayerView>,
) {
    view.cast_open = false;
    cx.notify();
}

fn heading(text: &'static str, theme: &Theme) -> Div {
    div()
        .px_3()
        .pt_2()
        .pb_1()
        .text_label()
        .text_color(theme.text_muted)
        .child(text)
}
