//! Casting a tile: playing its file on a TV or a speaker instead, from
//! where it is, with its tracks. While it casts, the tile's controls and
//! shortcuts drive the device and its bar follows the device; its own
//! playback waits, paused, and goes on from where the device was once the
//! cast ends.

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Corner, Div, ElementId, MouseDownEvent, Task, Window, div};
use stream_server::{Pick, SubtitleSource};

use super::PlayerView;
use crate::cast::{Casting, Device, Error, Link, Phase, Request, Shows, SubtitleChoice};
use crate::media::Source;
use crate::media::decoder::MediaInfo;
use crate::media::tracks::TrackKind;
use crate::settings::Settings;
use crate::ui::components::{Button, Icon, MenuRow, Panel, Popover};
use crate::ui::framing::Adjustments;
use crate::ui::theme::Theme;
use crate::ui::theme::motion::CAST_POLL;
use crate::ui::theme::player::{CAST_MENU_WIDTH, ERROR_MAX_WIDTH, PIP_ICON, TRACK_MENU_OFFSET};
use crate::ui::theme::text::TextStyle;

/// A tile's cast.
pub(super) struct Cast {
    link: Link,
    /// It has played there, so its ending is the device going away rather
    /// than it not starting.
    on: bool,
    /// Keeps the tile up to date with the device.
    _follow: Task<()>,
}

impl PlayerView {
    /// The device the tile casts to.
    pub fn casting(&self) -> Option<&Device> {
        self.cast.as_ref().map(|cast| cast.link.device())
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
        if self.player.error().is_some() {
            self.note("Can't cast a file that doesn't play", cx);
            return;
        }
        let info = self.player.info();
        if device.shows == Shows::Sound && info.as_ref().is_some_and(|info| info.has_video) {
            self.note(format!("{} plays only sound", device.name), cx);
            return;
        }
        let request = self.cast_request(info.as_ref(), start, autoplay, cx);
        // Another device this casts to lets go.
        self.cast = None;
        match Casting::link(cx, &device, request) {
            Ok(link) => {
                self.player.pause();
                let follow = cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(CAST_POLL).await;
                        let going = this.update(cx, |this, cx| this.follow_cast(cx));
                        if !matches!(going, Ok(true)) {
                            break;
                        }
                    }
                });
                self.cast = Some(Cast {
                    link,
                    on: false,
                    _follow: follow,
                });
            }
            Err(why) => self.note(format!("Can't play on {}: {why}", device.name), cx),
        }
    }

    /// What to cast: the file or stream, with the tracks playing here.
    fn cast_request(
        &self,
        info: Option<&MediaInfo>,
        start: f64,
        autoplay: bool,
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
            video: pick(info, TrackKind::Video),
            audio: pick(info, TrackKind::Audio),
            subtitles: info.and_then(|info| subtitle_choice(info, cx)),
            start,
            autoplay,
            volume: self.player.volume(),
            muted: self.player.is_muted(),
            looping: self.player.is_looping(),
        }
    }

    /// Play here again, from where the device was, playing if it was.
    /// Whether the tile was casting.
    pub fn end_cast(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(cast) = self.cast.take() else {
            return false;
        };
        let (position, playing) = (cast.link.position(), cast.link.is_playing());
        // The device stops, and its URL goes, on the link's thread.
        drop(cast);
        if !self.player.is_live() {
            self.player.seek(position);
        }
        if playing {
            self.player.play();
        }
        cx.notify();
        true
    }

    /// Stop the device as the app quits, handing back the thread stopping
    /// it, to wait for.
    pub fn stop_cast(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.cast.take().and_then(|cast| cast.link.stop())
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
            Phase::On { .. } => cast.on = true,
            Phase::Over(err) => {
                let note = match err {
                    Error::Gone if cast.on => format!("{name} went away"),
                    Error::Replaced if cast.on => format!("{name} plays something else now"),
                    err => format!("Can't play on {name}: {err}"),
                };
                self.end_cast(cx);
                self.note(note, cx);
                return false;
            }
        }
        cx.notify();
        true
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
            Some(cast) => cast.link.pause(),
            None => self.player.pause(),
        }
    }

    /// Seek to exactly `time`, here or there.
    pub fn seek(&mut self, time: f64) {
        match &mut self.cast {
            Some(cast) => cast.link.seek(time),
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
        match kind {
            TrackKind::Subtitle => cast.link.set_subtitles(subtitles),
            TrackKind::Video | TrackKind::Audio => cast.link.reload(
                pick(info.as_ref(), TrackKind::Video),
                pick(info.as_ref(), TrackKind::Audio),
            ),
        }
    }

    /// What fills the tile while it casts: where it plays, how the media
    /// gets there, what stays here, and a button to play it here again.
    pub(super) fn render_cast(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let cast = self.cast.as_ref()?;
        let device = cast.link.device();
        let (line, detail) = match cast.link.phase() {
            Phase::On { serves } => (
                format!("Playing on {}", device.name),
                Some(format!("{} · {serves}", device.kind)),
            ),
            _ => (format!("Connecting to {}…", device.name), None),
        };
        let settings = cx.global::<Settings>();
        let adjusted = self.adjustments != Adjustments::default() || settings.gamma() != [1.0; 2];
        let info = self.player.info();
        let picture_subtitles = info.as_ref().is_some_and(|info| {
            info.subtitle_track.is_some_and(|index| {
                info.tracks(TrackKind::Subtitle)
                    .any(|track| track.index == index && !track.is_text())
            })
        });
        let stays: Vec<&str> = [
            adjusted
                .then_some("Gamma, crop, rotation, flips and aspect ratio stay on this computer"),
            picture_subtitles.then_some("Picture subtitles stay on this computer"),
        ]
        .into_iter()
        .flatten()
        .collect();
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

    /// The cast button, while there are devices to cast to, and the menu
    /// of them it opens.
    pub(super) fn render_cast_button(
        &self,
        has_video: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut devices = Casting::devices_for(cx, has_video);
        if let Some(casting) = self.casting()
            && !devices.iter().any(|device| device.is(casting))
        {
            devices.push(casting.clone());
        }
        if devices.is_empty() {
            return None;
        }
        let button = Button::icon("cast", "cast")
            .overlay()
            .selected(self.cast.is_some())
            .on_click(cx.listener(|this, _, _, cx| {
                this.cast_open = !this.cast_open;
                cx.notify();
            }));
        Some(
            Popover::new("cast-backdrop", button)
                .anchor(Corner::BottomRight, TRACK_MENU_OFFSET)
                .when(self.cast_open, |this| {
                    this.menu(self.render_cast_menu(devices, theme, cx))
                })
                .on_dismiss(cx.listener(close_cast_menu))
                .into_any_element(),
        )
    }

    /// This computer, and the devices: the one playing checked.
    fn render_cast_menu(
        &self,
        devices: Vec<Device>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let casting = self.casting().cloned();
        let here = MenuRow::new("cast-here", "This Computer")
            .checked(casting.is_none())
            .mx_1()
            .on_click(cx.listener(|this, _, _, cx| this.cast_to(None, cx)));
        let rows = devices.into_iter().enumerate().map(|(i, device)| {
            let checked = casting.as_ref().is_some_and(|casting| casting.is(&device));
            MenuRow::new(
                ElementId::Name(format!("cast-{i}").into()),
                device.name.clone(),
            )
            .checked(checked)
            .detail(div().text_xs().child(device.kind))
            .mx_1()
            .on_click(cx.listener(move |this, _, _, cx| this.cast_to(Some(device.clone()), cx)))
        });
        Panel::new()
            .occlude()
            .flex()
            .flex_col()
            .w(CAST_MENU_WIDTH)
            .pb_1()
            .child(heading("Play On", theme))
            .child(here)
            .children(rows)
            .into_any_element()
    }
}

/// Which track of a kind to cast: the one playing here.
fn pick(info: Option<&MediaInfo>, kind: TrackKind) -> Pick {
    info.and_then(|info| info.selected(kind))
        .map_or(Pick::Best, Pick::Stream)
}

/// The subtitles showing, to send along: text ones, as devices don't show
/// pictures.
fn subtitle_choice(info: &MediaInfo, cx: &App) -> Option<SubtitleChoice> {
    let index = info.subtitle_track?;
    let (number, track) = info
        .tracks(TrackKind::Subtitle)
        .enumerate()
        .find(|(_, track)| track.index == index)?;
    if !track.is_text() {
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
