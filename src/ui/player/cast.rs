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
use crate::api::commands::{cast, output, window as windows};
use crate::app::{AudioEngine, output_kind};
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
use crate::ui::theme::motion::{CAST_POLL, NEXT_CAST_POLL};
use crate::ui::theme::player::{CAST_MENU_WIDTH, ERROR_MAX_WIDTH, PIP_ICON, TRACK_MENU_OFFSET};
use crate::ui::theme::text::TextStyle;

/// A tile's cast.
pub(super) struct Cast {
    pub(super) link: Link,
    /// It has played there, so its ending is the device going away rather
    /// than it not starting.
    on: bool,
    /// Only the sound goes; the picture plays here, following the device.
    pub(super) picture_here: bool,
    /// Keeps the tile up to date with the device.
    _follow: Task<()>,
}

/// A cast going on with the file just opened, once it's known what's in
/// it: whether it has sound for a speaker, a picture to keep here, and
/// subtitles all decide what goes.
pub(super) struct NextCast {
    pub(super) device: Device,
    pub(super) autoplay: bool,
    _wait: Task<()>,
}

impl PlayerView {
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
                    let player = this.playback.local();
                    if !player.is_known() && player.error().is_none() {
                        return false;
                    }
                    if let Some(next) = this.playback.next_cast.take() {
                        let start = this.playback.local().position();
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
        self.playback.next_cast = Some(NextCast {
            device,
            autoplay,
            _wait: wait,
        });
    }

    /// Close the menu, and cast to `device` (`cast.to`), or play here again
    /// (`cast.stop`).
    fn send_cast(&mut self, device: Option<&Device>, cx: &mut Context<Self>) {
        self.cast_open = false;
        cx.notify();
        self.send_run(cast::to(device), cx);
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
        self.playback.next_cast = None;
        if self.playback.local().error().is_some() {
            self.note("Can't cast a file that doesn't play", cx);
            return;
        }
        let info = self.playback.local().info();
        if device.shows == Shows::Sound && info.as_ref().is_some_and(|info| !info.has_audio) {
            self.note(
                format!("{} plays only sound, and this has none", device.name),
                cx,
            );
            return;
        }
        let picture_here =
            device.shows == Shows::Sound && info.as_ref().is_some_and(|info| info.has_video);
        let request = self.cast_request(info.as_deref(), start, autoplay, picture_here, cx);
        // Another device this casts to lets go; its sound is here again
        // until the new one plays it.
        self.playback.let_go_of_cast();
        match Casting::link(cx, &device, request) {
            Ok(link) => {
                if !picture_here {
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
                self.playback.hand_over(Cast {
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
        let player = self.playback.local();
        let source = match player.source() {
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
            volume: player.volume(),
            muted: player.is_muted(),
            looping: player.is_looping(),
            picture_here,
        }
    }

    /// Play here again, from where the device was, playing if it was.
    /// Whether the tile was casting.
    pub fn end_cast(&mut self, cx: &mut Context<Self>) -> bool {
        let ended = self.playback.end_cast();
        if ended {
            cx.notify();
        }
        ended
    }

    /// Stop the device as the app quits, handing back the thread stopping
    /// it, to wait for. The player here is left where the device was, so
    /// that's where it resumes.
    pub fn stop_cast(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.playback.stop_cast()
    }

    /// Show what the device does, say what went wrong, and end the cast
    /// once it's over. Whether it goes on.
    fn follow_cast(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(cast) = &mut self.playback.cast else {
            return false;
        };
        let name = cast.link.device().name.clone();
        if let Some(why) = cast.link.take_failure() {
            self.note(format!("{name}: {why}"), cx);
        }
        let Some(cast) = &mut self.playback.cast else {
            return false;
        };
        match cast.link.phase() {
            Phase::Connecting => {}
            Phase::On { .. } if !cast.on => {
                cast.on = true;
                if cast.picture_here {
                    self.note_change(format!("Sound on {name}"), cx);
                }
            }
            Phase::On { .. } => {}
            Phase::Over(Error::NeedsPin) => {
                let device = cast.link.device().clone();
                self.end_cast(cx);
                self.send_run(cast::pair(&device), cx);
                return false;
            }
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
        // While the slider is dragged the picture shows where the slider
        // is, the device seeking only once it's let go.
        if self.scrub.is_none() {
            self.playback.follow_sound();
        }
        cx.notify();
        true
    }

    /// What of the tile the device doesn't get, while it casts.
    #[cfg(test)]
    pub fn cast_stays_here(&self, cx: &App) -> Vec<&'static str> {
        self.playback
            .cast
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
            && self.playback.local().info().is_some_and(|info| {
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
        let cast = self
            .playback
            .cast
            .as_ref()
            .filter(|cast| !cast.picture_here)?;
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
                        .on_click(cx.listener(|this, _, _, cx| this.send_cast(None, cx))),
                )
                .into_any_element(),
        )
    }

    /// The cast button, while there are devices to cast to, displays or
    /// audio outputs to pick, and the menu of them it opens. It's lit while
    /// the tile plays elsewhere: cast to a device (or about to go on
    /// casting with the file just opened), or its window's sound playing
    /// through an output of its own.
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
            .selected(self.casting().is_some() || self.window_sound.is_some())
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
    /// the window full screen there (`window.show_on_display`), or plays the tile
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
                .on_click(cx.listener(|this, _, _, cx| this.send_cast(None, cx)))
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
                window.dispatch_action(Box::new(windows::show_on_display(screen.id)), cx);
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
            .on_click(cx.listener(move |this, _, _, cx| this.send_cast(Some(&device), cx)))
        });
        Panel::new()
            .occlude()
            .flex()
            .flex_col()
            .w(CAST_MENU_WIDTH)
            .pb_1()
            .when(play_on, |this| {
                this.child(heading("Play This Tile On", theme))
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
                this.child(heading("Play This Window's Sound On", theme))
                    .children(sound)
            })
            .into_any_element()
    }

    /// The app's output, checked unless the window plays through one of its
    /// own; then `outputs`, the window's own checked, and listed too while
    /// it isn't connected.
    /// Picking one plays the whole window through it
    /// (`output.set_window_device`).
    fn render_window_sound(
        &self,
        outputs: Vec<DeviceInfo>,
        cx: &mut Context<Self>,
    ) -> Vec<MenuRow> {
        let own = self.window_sound.clone();
        let mut rows = vec![
            MenuRow::new("sound-app", output::SAME_AS_APP)
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
            let mut row = MenuRow::new(
                ElementId::Name(format!("sound-{i}").into()),
                device.name.clone(),
            )
            .checked(checked);
            // As the devices above say theirs: a speaker also cast to, with
            // DLNA say, is told apart.
            if let Some(kind) = output_kind(&device.id) {
                row = row.detail(
                    div()
                        .debug_selector(move || format!("sound-{i}-kind"))
                        .text_xs()
                        .child(kind),
                );
            }
            rows.push(row.mx_1().on_click(pick_window_sound(Some(device), cx)));
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
        window.dispatch_action(Box::new(output::set_window_device(device.as_ref())), cx);
        cx.notify();
    }))
}

/// Which track of a kind to cast: the one playing here.
pub(super) fn pick(info: Option<&MediaInfo>, kind: TrackKind) -> Pick {
    info.and_then(|info| info.selected(kind))
        .map_or(Pick::Best, Pick::Stream)
}

/// The subtitles showing, to send along: text ones, and picture ones in
/// the media, which the stream server draws into video it re-encodes.
/// Picture subtitles from a file of their own stay here.
pub(super) fn subtitle_choice(info: &MediaInfo, cx: &App) -> Option<SubtitleChoice> {
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
