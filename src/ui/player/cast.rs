//! Casting a block: playing its file on a TV or a speaker instead, from
//! where it is, with its tracks. While it casts, the block's controls and
//! shortcuts drive the device and its bar follows the device; its own
//! playback waits, paused, and goes on from where the device was once the
//! cast ends. A video cast to a speaker sends only its sound, and the
//! picture plays on here, following the speaker.
//!
//! The cast button's menu says where the block plays. Play Video On: the
//! displays, with more than one, to put the window full screen on, as
//! Video → Output Device does on macOS, and the devices that show video.
//! Play Audio On: with more than one audio output, those the block can
//! play its sound through instead of the app's, as Audio → Block Output
//! Device does (the only place to pick them on Linux, which has no menu
//! bar), and the speakers to cast its sound to.

use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Context, Corner, Div, ElementId, MouseDownEvent, Task, Window, div,
};
use stream_server::{Pick, SubtitleSource};

use super::PlayerView;
use crate::api::commands::{cast, output, window as windows};
use crate::app::{AudioEngine, Held, airplay_id, hold_receiver, output_kind};
use crate::cast::airplay;
use crate::cast::target::FOLLOW_POLL;
use crate::cast::{Casting, Changes, Device, Error, Link, Phase, Request, Shows, SubtitleChoice};
use crate::media::Source;
use crate::media::decoder::MediaInfo;
use crate::media::details::VideoDetails;
use crate::media::output::DeviceInfo;
use crate::media::tracks::TrackKind;
use crate::settings::{AudioDevice, Settings};
use crate::ui::components::{Button, Icon, MenuRow, Panel, Popover};
use crate::ui::displays::{self, DisplayInfo};
use crate::ui::framing::Adjustments;
use crate::ui::theme::Theme;
use crate::ui::theme::motion::{CAST_POLL, NEXT_CAST_POLL};
use crate::ui::theme::player::{CAST_MENU_WIDTH, ERROR_MAX_WIDTH, PIP_ICON, TRACK_MENU_OFFSET};
use crate::ui::theme::text::TextStyle;

/// What a Bluetooth output says it is.
const BLUETOOTH: &str = "Bluetooth";

/// A block's cast.
pub(super) struct Cast {
    pub(super) link: Link,
    /// It has played there, so its ending is the device going away rather
    /// than it not starting.
    on: bool,
    /// Only the sound goes; the picture plays here, following the device.
    pub(super) picture_here: bool,
    /// The AirPlay receiver it's cast to, which no output plays through
    /// meanwhile; let go of once the link is.
    _held: Option<Held>,
    /// Keeps the block up to date with the device.
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

    /// Cast to `device` from where the block is, playing if it plays; or
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
        // A receiver plays one thing at a time: outputs playing through it
        // let go of it first, rather than take it back from the cast.
        let held = if device.kind == airplay::KIND {
            hold_receiver(&device.id, cx)
        } else {
            None
        };
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
                    _held: held,
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
            changes: self.cast_changes(info, picture_here, cx),
        }
    }

    /// What the block does to what it plays, for a device playing its
    /// picture to do too: its delays and its picture's adjustments. With
    /// the picture here, it all happens here.
    fn cast_changes(&self, info: Option<&MediaInfo>, picture_here: bool, cx: &App) -> Changes {
        if picture_here {
            return Changes::default();
        }
        let player = self.playback.local();
        Changes {
            audio_delay: player.audio_delay(),
            subtitle_delay: player.subtitle_delay(),
            adjustments: info.and_then(|info| {
                picture_adjustments(&self.adjustments, info, cx.global::<Settings>())
            }),
        }
    }

    /// Have a device playing the picture do what the block now does to
    /// it: its delays, adjustments and gamma.
    pub(super) fn send_cast_changes(&mut self, cx: &App) {
        let info = self.playback.local().info();
        let changes = self.cast_changes(info.as_deref(), false, cx);
        self.playback.set_cast_changes(changes);
    }

    /// Play here again, from where the device was, playing if it was.
    /// Whether the block was casting.
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

    /// What of the block the device doesn't get, while it casts.
    #[cfg(test)]
    pub fn cast_stays_here(&self, cx: &App) -> Vec<&'static str> {
        self.playback
            .cast
            .as_ref()
            .map_or_else(Vec::new, |cast| self.stays_here(&cast.link.phase(), cx))
    }

    fn stays_here(&self, phase: &Phase, cx: &App) -> Vec<&'static str> {
        // What goes is known once it plays there.
        let Phase::On {
            draws_subtitles: drawn,
            adjusts_picture: adjusted,
            delays_sound: delayed,
            ..
        } = *phase
        else {
            return Vec::new();
        };
        let info = self.playback.local().info();
        let adjusts = info.as_ref().is_some_and(|info| {
            picture_adjustments(&self.adjustments, info, cx.global::<Settings>()).is_some()
        });
        let picture_subtitles = !drawn
            && info.as_ref().is_some_and(|info| {
                info.subtitle_track.is_some_and(|index| {
                    info.tracks(TrackKind::Subtitle)
                        .any(|track| track.index == index && !track.is_text())
                })
            });
        let delays = self.playback.local().audio_delay() != 0.0;
        [
            (adjusts && !adjusted)
                .then_some("Gamma, crop, rotation, flips and aspect ratio stay on this computer"),
            picture_subtitles.then_some("Picture subtitles stay on this computer"),
            (delays && !delayed).then_some("The audio delay stays on this computer"),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// What fills the block while it casts: where it plays, how the media
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

    /// The cast button, while there are displays, devices or outputs to
    /// play the block on, and the menu of them it opens. It's lit while the
    /// block plays elsewhere (`plays_elsewhere`).
    pub(super) fn render_cast_button(
        &self,
        info: Option<&MediaInfo>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let choices = self.choices(info, window, cx);
        if choices.is_empty() {
            return None;
        }
        let button = Button::icon("cast", "cast")
            .overlay()
            .selected(self.plays_elsewhere(cx))
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
                    this.menu(self.render_cast_menu(choices, theme, cx))
                })
                .on_dismiss(cx.listener(close_cast_menu))
                .into_any_element(),
        )
    }

    /// Whether the block plays elsewhere: cast to a device (or about to go
    /// on casting with the file just opened), through an output of its own,
    /// or through the app's, an AirPlay speaker.
    fn plays_elsewhere(&self, cx: &App) -> bool {
        let app = cx.global::<Settings>().audio_output.device.as_ref();
        self.casting().is_some()
            || self.sound.is_some()
            || app.is_some_and(|device| output_kind(&device.id).is_some())
    }

    /// What its menu lists, as things are now.
    fn choices(&self, info: Option<&MediaInfo>, window: &Window, cx: &App) -> Choices {
        // Until the file says, it may well have sound.
        let has_audio = info.is_none_or(|info| info.has_audio);
        let has_video = info.is_some_and(|info| info.has_video);
        let mut devices = Casting::devices_for(cx, has_audio);
        if let Some(casting) = self.casting()
            && !devices.iter().any(|device| device.is(casting))
        {
            devices.push(casting.clone());
        }
        // Numbered among them all, in both lists.
        let (video, sound) = devices
            .into_iter()
            .enumerate()
            .partition(|(_, device)| has_video && device.shows == Shows::Video);
        let mut screens = if has_video {
            displays::connected(cx)
        } else {
            Vec::new()
        };
        if screens.len() < 2 {
            screens.clear();
        }
        let here = (!screens.is_empty())
            .then(|| displays::of(window, cx))
            .flatten();
        let outputs = AudioEngine::devices(cx);
        let outputs = if has_audio && (outputs.len() > 1 || self.sound.is_some()) {
            outputs.to_vec()
        } else {
            Vec::new()
        };
        Choices {
            screens,
            here,
            video,
            sound,
            outputs,
        }
    }

    /// Where its picture plays (`render_video_on`), while it has one to play
    /// elsewhere, then where its sound does (`render_audio_on`).
    fn render_cast_menu(
        &self,
        choices: Choices,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Choices {
            screens,
            here,
            video,
            sound,
            outputs,
        } = choices;
        let picture = (!video.is_empty() || !screens.is_empty())
            .then(|| self.render_video_on(screens, here, video, theme, cx));
        let audio = (!outputs.is_empty() || !sound.is_empty())
            .then(|| self.render_audio_on(outputs, sound, cx));
        Panel::new()
            .occlude()
            .flex()
            .flex_col()
            .w(CAST_MENU_WIDTH)
            .pb_1()
            .when_some(picture, |this, rows| {
                this.child(heading("Play Video On", theme)).children(rows)
            })
            .when_some(audio, |this, rows| {
                this.child(heading("Play Audio On", theme)).children(rows)
            })
            .into_any_element()
    }

    /// This computer, or each display (`screens`) with the window's
    /// (`here`) checked, and the devices that show the picture (`devices`),
    /// the one it's cast to checked. Picking a display puts the window full
    /// screen there (`window.show_on_display`), or plays the block here
    /// again while it casts, if the window is on it; Wayland may keep it on
    /// its own display, which the menu says.
    fn render_video_on(
        &self,
        screens: Vec<DisplayInfo>,
        here: Option<u32>,
        devices: Vec<(usize, Device)>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let picture = self
            .casting()
            .filter(|device| device.shows == Shows::Video)
            .cloned();
        let mut rows = Vec::new();
        if screens.is_empty() {
            let row = MenuRow::new("cast-here", "This Computer").checked(picture.is_none());
            let row = match picture {
                Some(_) => row.on_click(cx.listener(|this, _, _, cx| this.send_cast(None, cx))),
                None => row.on_click(cx.listener(close_menu)),
            };
            rows.push(row.mx_1().into_any_element());
        }
        let wayland = !screens.is_empty() && displays::placed_by_compositor();
        for (i, screen) in screens.into_iter().enumerate() {
            let row = MenuRow::new(
                ElementId::Name(format!("display-{i}").into()),
                screen.name.clone(),
            )
            .checked(picture.is_none() && here == Some(screen.id))
            .detail(div().text_xs().child("Full screen"))
            .mx_1()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.cast_open = false;
                window.dispatch_action(Box::new(windows::show_on_display(screen.id)), cx);
                cx.notify();
            }));
            rows.push(row.into_any_element());
        }
        if wayland {
            rows.push(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Wayland may keep it where it is")
                    .into_any_element(),
            );
        }
        for (i, device) in devices {
            let checked = picture.as_ref().is_some_and(|picture| picture.is(&device));
            rows.push(cast_row(i, device, checked, cx));
        }
        rows
    }

    /// The app's output, saying which it is, then `outputs`, its own
    /// checked, and listed too while it isn't connected; then the devices
    /// that play its sound alone (`devices`). The one it's cast to is
    /// checked instead, its picture with it or not, and listed while it
    /// isn't an output. Picking an output plays the block here, through it
    /// (`output.set_block_device`).
    fn render_audio_on(
        &self,
        outputs: Vec<DeviceInfo>,
        devices: Vec<(usize, Device)>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let casting = self.casting().cloned();
        let own = self.sound.clone().filter(|_| casting.is_none());
        let app = cx.global::<Settings>().audio_output.device.as_ref();
        let app = app.map_or_else(|| "System Default".to_owned(), |app| app.name.clone());
        let same = casting.is_none() && own.is_none();
        let mut rows = vec![sound_row(
            MenuRow::new("sound-app", output::SAME_AS_APP).detail(
                div()
                    .debug_selector(|| "sound-app-output".into())
                    .text_xs()
                    .child(app),
            ),
            None,
            same,
            cx,
        )];
        let missing = own
            .clone()
            .filter(|own| !outputs.iter().any(|output| output.id == own.id));
        let mut cast_listed = false;
        for (i, output) in outputs.into_iter().enumerate() {
            let cast_here = casting
                .as_ref()
                .is_some_and(|cast| is_cast_to(&output.id, cast));
            cast_listed |= cast_here;
            let checked = cast_here || own.as_ref().is_some_and(|own| own.id == output.id);
            let mut row = MenuRow::new(
                ElementId::Name(format!("sound-{i}").into()),
                output.name.clone(),
            );
            // As the devices say theirs: a speaker also cast to, with DLNA
            // say, is told apart.
            if let Some(kind) = says(&output) {
                row = row.detail(
                    div()
                        .debug_selector(move || format!("sound-{i}-kind"))
                        .text_xs()
                        .child(kind),
                );
            }
            let device = AudioDevice {
                id: output.id,
                name: output.name,
            };
            rows.push(sound_row(row, Some(device), checked, cx));
        }
        if let Some(device) = missing {
            let row = MenuRow::new("sound-missing", device.name.clone())
                .detail(div().text_xs().child("Not connected"));
            rows.push(sound_row(row, Some(device), true, cx));
        }
        for (i, device) in devices {
            let checked = casting.as_ref().is_some_and(|cast| cast.is(&device));
            cast_listed |= checked;
            rows.push(cast_row(i, device, checked, cx));
        }
        if let Some(cast) = casting.filter(|_| !cast_listed) {
            let row = MenuRow::new("sound-cast", cast.name.clone())
                .checked(true)
                .detail(div().text_xs().child(cast.kind))
                .mx_1()
                .on_click(cx.listener(close_menu));
            rows.push(row.into_any_element());
        }
        rows
    }
}

/// What a block's menu lists.
struct Choices {
    /// The displays to put the window full screen on, while there are two
    /// or more and the block has a picture.
    screens: Vec<DisplayInfo>,
    /// The display the window is on, while they're listed.
    here: Option<u32>,
    /// The devices that show its picture, and play its sound with it,
    /// numbered among all it can be cast to.
    video: Vec<(usize, Device)>,
    /// Those that play its sound alone: speakers, or for a block without a
    /// picture, any.
    sound: Vec<(usize, Device)>,
    /// The outputs to pick from, while there's a choice.
    outputs: Vec<DeviceInfo>,
}

impl Choices {
    fn is_empty(&self) -> bool {
        self.screens.is_empty()
            && self.video.is_empty()
            && self.sound.is_empty()
            && self.outputs.is_empty()
    }
}

/// What `output` says it is, on the right of its row: the protocol it's
/// reached by, or Bluetooth. This computer's others, built in or wired,
/// say nothing.
fn says(output: &DeviceInfo) -> Option<&'static str> {
    output_kind(&output.id).or(output.bluetooth.then_some(BLUETOOTH))
}

/// Whether output `id` is the device `cast`, as an AirPlay TV is both.
fn is_cast_to(id: &str, cast: &Device) -> bool {
    output_kind(id) == Some(cast.kind) && airplay_id(id) == Some(cast.id.as_str())
}

/// Device `device`, the `i`th the block can be cast to, `checked` while it
/// is; picking it casts the block there.
fn cast_row(i: usize, device: Device, checked: bool, cx: &mut Context<PlayerView>) -> AnyElement {
    MenuRow::new(
        ElementId::Name(format!("cast-{i}").into()),
        device.name.clone(),
    )
    .checked(checked)
    .detail(div().text_xs().child(device.kind))
    .mx_1()
    .on_click(cx.listener(move |this, _, _, cx| this.send_cast(Some(&device), cx)))
    .into_any_element()
}

/// `row`, for `device` (or with `None`, the app's output), `checked` while
/// the block plays there. Picking it, unchecked, plays the block here
/// through it: one cast plays here again first.
fn sound_row(
    row: MenuRow,
    device: Option<AudioDevice>,
    checked: bool,
    cx: &mut Context<PlayerView>,
) -> AnyElement {
    let row = row.checked(checked).mx_1();
    if checked {
        return row.on_click(cx.listener(close_menu)).into_any_element();
    }
    row.on_click(cx.listener(move |view, _, _, cx| {
        view.cast_open = false;
        if view.casting().is_some() {
            view.send_run(cast::to(None), cx);
        }
        view.send_run(output::set_block_device(device.as_ref()), cx);
        cx.notify();
    }))
    .into_any_element()
}

/// Clicks on what's picked already close the menu, and do nothing else.
fn close_menu(view: &mut PlayerView, _: &ClickEvent, _: &mut Window, cx: &mut Context<PlayerView>) {
    view.cast_open = false;
    cx.notify();
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

/// What the viewer does to the picture, for a device re-encoding it to do
/// too: turned and mirrored after the file's own orientation, at its
/// aspect ratio and crop, and bent by the gamma for its kind of video.
/// `None` while it shows as it is, or without a picture.
fn picture_adjustments(
    adjustments: &Adjustments,
    info: &MediaInfo,
    settings: &Settings,
) -> Option<stream_server::Adjustments> {
    if !info.has_video {
        return None;
    }
    // The gamma for the video as it shows here.
    let hdr = info.video_details.as_ref().is_some_and(VideoDetails::hdr);
    let gamma = settings.gamma()[usize::from(hdr)];
    if *adjustments == Adjustments::default() && gamma == 1.0 {
        return None;
    }
    let orientation = adjustments.orientation.after(info.orientation);
    Some(stream_server::Adjustments {
        turns: orientation.clockwise_turns(),
        mirrored: orientation.is_mirrored(),
        aspect: adjustments.aspect.map(|ratio| f64::from(ratio.value)),
        crop: adjustments.crop.map(|ratio| f64::from(ratio.value)),
        gamma: f64::from(gamma),
    })
}

#[cfg(test)]
#[path = "tests/cast.rs"]
mod tests;
