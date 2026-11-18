use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnchoredPositionMode, AnyElement, ClickEvent, Context, Corner, Div, EventEmitter, FontWeight,
    Hsla, MouseButton, ObjectFit, SharedString, Window, anchored, black, deferred, div, img,
    linear_color_stop, linear_gradient, point, px, relative, white,
};

use super::TRAFFIC_LIGHTS_WIDTH;
use super::components::{Icon, IconButton, Panel, Slide, Slider, SliderEvent};
use super::subtitles;
use super::theme::{Theme, overlay};
use crate::media::decoder::MediaInfo;
use crate::media::picture::PictureId;
use crate::media::subtitles::CueContent;
use crate::media::tracks::TrackKind;
use crate::media::{Mixer, Picture, Player, PlayerOptions, Source};
use crate::settings::{ControlsVisibility, Settings, VideoFit};

/// Frames that were on screen wait here for a few display frames before they
/// are released, so a frame still in flight on the GPU keeps its texture (or,
/// for a surface, its buffer isn't refilled with a newer frame).
#[derive(Clone, Default)]
pub struct Graveyard(Rc<RefCell<Vec<Buried>>>);

/// A retired frame and when it went off screen.
type Buried = (Picture, Instant);

const RELEASE_DELAY: Duration = Duration::from_millis(60);

impl Graveyard {
    pub fn bury(&self, picture: Picture) {
        self.0.borrow_mut().push((picture, Instant::now()));
    }

    /// Release frames that have been off screen long enough. Returns whether any are left.
    pub fn collect(&self, window: &mut Window) -> bool {
        let mut pictures = self.0.borrow_mut();
        pictures.retain(|(picture, buried)| {
            let expired = buried.elapsed() >= RELEASE_DELAY;
            // Images live in gpui's texture atlas until dropped from it;
            // surfaces only need their last reference to go.
            if expired && let Picture::Image(image) = picture {
                let _ = window.drop_image(image.clone());
            }
            !expired
        });
        !pictures.is_empty()
    }
}

/// How long a note on what a shortcut did shows.
const OSD_DURATION: Duration = Duration::from_millis(1200);
/// How long, at the end of that, it takes to fade out.
const OSD_FADE: Duration = Duration::from_millis(200);
/// How long the track reel takes to roll to the track picked.
const REEL_ROLL: Duration = Duration::from_millis(220);
/// How tall a track's row is, in the menu and on the reel.
const TRACK_ROW: f32 = 26.;
/// How bright the tracks before and after the one picked are on the reel.
const REEL_FADED: f32 = 0.4;

/// The note over a tile on what a shortcut did.
enum Osd {
    Text(SharedString),
    /// A track picked with a shortcut.
    Reel(TrackReel),
}

/// The tracks of a kind on a reel, like a slot machine's: the one picked
/// in the middle, the ones before and after it faded above and below.
struct TrackReel {
    /// In the order shortcuts go through them.
    choices: Vec<TrackChoice>,
    /// The one picked.
    at: usize,
}

/// A track to pick, in the menu or on the reel.
struct TrackChoice {
    /// `None` for no subtitles.
    index: Option<usize>,
    label: String,
    /// Its codec and format.
    detail: Option<String>,
}

fn track_heading(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Video => "Video",
        TrackKind::Audio => "Audio",
        TrackKind::Subtitle => "Subtitles",
    }
}

/// A kind's tracks, and for subtitles "Off" before them.
fn track_choices(info: &MediaInfo, kind: TrackKind) -> Vec<TrackChoice> {
    let off = (kind == TrackKind::Subtitle).then(|| TrackChoice {
        index: None,
        label: "Off".to_owned(),
        detail: None,
    });
    off.into_iter()
        .chain(info.tracks(kind).enumerate().map(|(i, track)| TrackChoice {
            index: Some(track.index),
            label: track.label(i + 1),
            detail: Some(track.detail()),
        }))
        .collect()
}

pub enum PlayerEvent {
    Close,
    /// Clicked: shortcuts go to this tile now.
    Activate,
    /// Fill the grid with this tile.
    ToggleMaximize,
    /// Move to a new window.
    PopOut,
}

pub struct PlayerView {
    player: Player,
    title: SharedString,
    graveyard: Graveyard,
    seek_drag: Rc<Cell<bool>>,
    volume_drag: Rc<Cell<bool>>,
    /// Position shown while the seek slider is being dragged.
    scrub: Option<f64>,
    resume_after_scrub: bool,
    level: f32,
    /// The frame handed to gpui in the last render (and so maybe in use on the GPU).
    drawn: Option<PictureId>,
    /// The subtitles handed to gpui in the last render.
    cues: Vec<Arc<CueContent>>,
    /// The audio and subtitle track menu is open.
    tracks_open: bool,
    /// A note on what a shortcut did, and when it was shown.
    osd: Option<(Osd, Instant)>,
    /// The subtitles hidden with a shortcut, to show again.
    hidden_subtitles: Option<usize>,
    /// Shortcuts go to this tile, which is outlined when there are others.
    pub active: bool,
    pub maximized: bool,
    /// The only tile in its window: there is nowhere to expand it to, and
    /// popping it out would leave the window empty.
    pub alone: bool,
    /// In its window's top-left corner, where the macOS traffic lights are.
    pub top_left: bool,
    /// The controls show only while the mouse is over the tile, which needs
    /// the mouse in the window.
    pub mouse_in_window: bool,
}

impl EventEmitter<PlayerEvent> for PlayerView {}

impl PlayerView {
    pub fn new(
        source: Source,
        mixer: Option<Arc<Mixer>>,
        settings: &Settings,
        graveyard: Graveyard,
    ) -> Self {
        let title = source.name().into();
        let player = Player::open(
            source,
            mixer,
            PlayerOptions {
                autoplay: settings.autoplay,
                looping: settings.loop_by_default,
                volume: settings.default_volume,
                max_short_side: settings.max_resolution.short_side(),
                hardware_decoding: settings.hardware_decoding,
            },
        );
        Self {
            player,
            title,
            graveyard,
            seek_drag: Rc::default(),
            volume_drag: Rc::default(),
            scrub: None,
            resume_after_scrub: false,
            level: 0.0,
            drawn: None,
            cues: Vec::new(),
            tracks_open: false,
            osd: None,
            hidden_subtitles: None,
            active: false,
            maximized: false,
            alone: true,
            top_left: false,
            mouse_in_window: true,
        }
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn player_mut(&mut self) -> &mut Player {
        &mut self.player
    }

    /// How pictures look changed (gamma). Surfaces take it as they are
    /// drawn and new frames as they are converted, so only a paused video
    /// showing a converted image needs its frame decoded again.
    pub fn picture_settings_changed(&mut self) {
        if !self.player.is_playing() && matches!(self.player.frame(), Some(Picture::Image(_))) {
            self.player.refresh();
        }
    }

    /// Release a frame that is no longer current. Only frames that were rendered
    /// can be in use on the GPU; the rest are simply dropped.
    fn retire(&mut self, picture: Picture) {
        if self.drawn == Some(picture.id()) {
            self.drawn = None;
            self.graveyard.bury(picture);
        }
    }

    /// Show `cues` from now on. Bitmap subtitles that go off screen are
    /// released like frames.
    fn show_cues(&mut self, cues: Vec<Arc<CueContent>>) {
        for old in std::mem::replace(&mut self.cues, cues) {
            if !self.cues.iter().any(|cue| Arc::ptr_eq(cue, &old)) {
                self.bury_cue(&old);
            }
        }
    }

    fn bury_cue(&self, cue: &CueContent) {
        for bitmap in &cue.bitmaps {
            self.graveyard.bury(Picture::Image(bitmap.image.clone()));
        }
    }

    /// Keeps playback moving (frame queues, end of file, looping) while the
    /// window isn't drawing, for example when it is minimized.
    pub fn background_tick(&mut self, cx: &mut Context<Self>) {
        let was_playing = self.player.is_playing();
        let tick = self.player.tick();
        let advanced = tick.retired.is_some();
        if let Some(picture) = tick.retired {
            self.retire(picture);
        }
        if advanced || was_playing != self.player.is_playing() {
            cx.notify();
        }
    }

    /// Move this tile to another window, whose frames are released through
    /// `graveyard`.
    pub fn move_to_window(&mut self, graveyard: Graveyard) {
        // Pictures drawn so far are in the old window's texture atlas, so the
        // old window releases them; the new one uploads its own copies.
        let cover = self.player.cover().map(Picture::Image);
        for picture in self.player.frame().into_iter().chain(cover) {
            self.graveyard.bury(picture);
        }
        for cue in std::mem::take(&mut self.cues) {
            self.bury_cue(&cue);
        }
        self.graveyard = graveyard;
        self.maximized = false;
    }

    /// Hand every frame this tile drew to the graveyard; call before dropping it.
    pub fn release_pictures(&mut self) {
        for picture in self.player.take_pictures() {
            self.graveyard.bury(picture);
        }
        for cue in std::mem::take(&mut self.cues) {
            self.bury_cue(&cue);
        }
    }

    fn on_surface_click(
        &mut self,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Single click toggles playback. A double click toggles twice, which
        // leaves playback as it was, and expands the tile.
        self.player.toggle();
        if event.click_count() == 2 {
            self.toggle_expanded(window, cx);
        }
        cx.notify();
    }

    /// Expand the tile to fill its window, or the window to fill the screen
    /// when the tile is all it shows; or undo that.
    fn toggle_expanded(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.alone {
            window.toggle_fullscreen();
        } else {
            cx.emit(PlayerEvent::ToggleMaximize);
        }
    }

    fn on_seek(&mut self, event: &SliderEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(duration) = self.player.duration() else {
            return;
        };
        let time = event.value as f64 * duration;
        match event.slide {
            Slide::Start => {
                self.resume_after_scrub = self.player.is_playing();
                self.player.pause();
                self.scrub = Some(time);
                self.player.seek(time);
            }
            Slide::Move => {
                self.scrub = Some(time);
                self.player.seek(time);
            }
            Slide::End => {
                self.scrub = None;
                self.player.seek(time);
                if self.resume_after_scrub {
                    self.player.play();
                }
            }
        }
        cx.notify();
    }

    fn on_volume(&mut self, event: &SliderEvent, _: &mut Window, cx: &mut Context<Self>) {
        let value = event.value;
        self.player.set_volume(value);
        if value > 0.0 && self.player.is_muted() {
            self.player.set_muted(false);
        }
        cx.notify();
    }

    /// Show a note on what a shortcut did.
    pub fn note(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.osd = Some((Osd::Text(text.into()), Instant::now()));
        cx.notify();
    }

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

    /// Whether the file has tracks to pick from: subtitles, or more than one
    /// video or audio track.
    fn has_track_choice(info: &MediaInfo) -> bool {
        info.tracks(TrackKind::Subtitle).next().is_some()
            || info.tracks(TrackKind::Audio).count() > 1
            || info.tracks(TrackKind::Video).count() > 1
    }

    /// The menu of tracks: a column each for audio and subtitles (when the
    /// file has any) and video (when it has more than one), each scrolling
    /// on its own, so a long list of subtitles leaves audio in view.
    fn render_tracks_menu(
        &self,
        info: &MediaInfo,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut menu = Panel::new().occlude().flex();
        let mut first = true;
        for kind in [TrackKind::Video, TrackKind::Audio, TrackKind::Subtitle] {
            let heading = track_heading(kind);
            let choices = track_choices(info, kind);
            let tracks = choices.iter().filter(|c| c.index.is_some()).count();
            let listed = match kind {
                TrackKind::Video => tracks > 1,
                _ => tracks > 0,
            };
            if !listed {
                continue;
            }
            let selected = info.selected(kind);
            let mut list = div().id(heading).max_h(px(320.)).overflow_y_scroll().pb_1();
            for choice in &choices {
                let index = choice.index;
                list = list.child(
                    track_row(choice, selected == index, theme.text_muted)
                        .id(("track", index.map_or(0, |i| i + 1)))
                        .group("track-row")
                        .mx_1()
                        .cursor_pointer()
                        .hover(|style| style.bg(theme.accent).text_color(white()))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.player.select_track(kind, index);
                            this.tracks_open = false;
                            cx.notify();
                        })),
                );
            }
            menu = menu.child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(240.))
                    .when(!first, |this| this.border_l_1().border_color(theme.border))
                    .child(track_heading_row(heading, theme))
                    .child(list),
            );
            first = false;
        }
        menu.into_any_element()
    }

    fn render_content(&self, theme: &Theme, fit: ObjectFit) -> AnyElement {
        if let Some(error) = self.player.error() {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .max_w(px(420.))
                .px_4()
                .text_center()
                .child(div().text_sm().text_color(theme.danger).child(
                    if self.player.is_network() {
                        "Can't play this stream"
                    } else {
                        "Can't play this file"
                    },
                ))
                .child(div().text_xs().text_color(theme.text_muted).child(error))
                .into_any_element();
        }
        if let Some(frame) = self.player.frame() {
            return match frame {
                Picture::Image(image) => img(image).size_full().object_fit(fit).into_any_element(),
                #[cfg(target_os = "macos")]
                Picture::Surface(surface) => gpui::surface(surface.for_drawing())
                    .size_full()
                    .object_fit(fit)
                    .into_any_element(),
            };
        }
        match self.player.info() {
            Some(info) if !info.has_video => self.render_audio(theme),
            _ => div()
                .text_sm()
                .text_color(theme.text_muted)
                .child(if self.player.is_network() {
                    "Connecting…"
                } else {
                    "Opening…"
                })
                .into_any_element(),
        }
    }

    /// Audio-only files get their cover art (or a note) and a level meter.
    fn render_audio(&self, theme: &Theme) -> AnyElement {
        let artwork = match self.player.cover() {
            Some(cover) => img(cover)
                .size(px(160.))
                .object_fit(ObjectFit::Contain)
                .rounded(px(8.))
                .into_any_element(),
            None => div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(96.))
                .rounded_full()
                .bg(theme.surface_hover)
                .child(Icon::new("music").size(px(40.)).color(theme.accent))
                .into_any_element(),
        };
        let title = self
            .player
            .info()
            .and_then(|i| i.title)
            .map(SharedString::from)
            .unwrap_or_else(|| self.title.clone());
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .px_4()
            .child(artwork)
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_sm()
                    .text_color(theme.text)
                    .child(title),
            )
            .child(
                div()
                    .w(px(160.))
                    .h(px(4.))
                    .rounded_full()
                    .bg(theme.track)
                    .child(
                        div()
                            .h_full()
                            .w(relative(self.level.clamp(0.0, 1.0)))
                            .rounded_full()
                            .bg(theme.accent),
                    ),
            )
            .into_any_element()
    }
}

impl Render for PlayerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tick = self.player.tick();
        if let Some(picture) = tick.retired {
            self.retire(picture);
        }
        self.drawn = self.player.frame().map(|frame| frame.id());
        let cues = self.player.subtitles();
        self.show_cues(cues);
        let level = self.player.level();
        self.level = if level > self.level {
            level
        } else {
            self.level * 0.88 + level * 0.12
        };
        if tick.animate || self.level > 0.005 {
            window.request_animation_frame();
        }

        let theme = Theme::get(window, cx);
        let settings = cx.global::<Settings>();
        let fit = match settings.video_fit {
            VideoFit::Contain => ObjectFit::Contain,
            VideoFit::Cover => ObjectFit::Cover,
        };
        let cover = settings.video_fit == VideoFit::Cover;
        let subtitle_style = settings.subtitles.clone();
        let show_titles = settings.show_titles;
        let info = self.player.info();
        let has_video = self.player.frame().is_some();
        let dragging = self.seek_drag.get() || self.volume_drag.get();
        let pinned = settings.controls == ControlsVisibility::Always
            || dragging
            || self.tracks_open
            || !self.player.is_playing()
            || self.player.error().is_some();

        let expanded = if self.alone {
            window.is_fullscreen()
        } else {
            self.maximized
        };
        // Full screen hides the traffic lights.
        let under_traffic_lights =
            cfg!(target_os = "macos") && self.top_left && !window.is_fullscreen();

        let group: SharedString = format!("tile-{}", cx.entity_id()).into();
        let duration = self.player.duration();
        let live = info.as_ref().is_some_and(|i| i.live);
        // A stream that has started and then stalls; opening has its own message.
        let stalled = self.player.is_buffering()
            && self.player.is_network()
            && (has_video || info.as_ref().is_some_and(|i| !i.has_video));
        let position = self.scrub.unwrap_or_else(|| self.player.position());
        let progress = duration
            .filter(|d| *d > 0.0)
            .map_or(0.0, |d| (position / d) as f32);
        let time_label = match duration {
            Some(d) => format!("{} / {}", format_time(position), format_time(d)),
            None => format_time(position),
        };
        let details = info.as_ref().map(|info| {
            let mut parts = Vec::new();
            if let Some((w, h)) = info.video_size {
                parts.push(format!("{w}×{h}"));
            }
            if let Some(fps) = info.frame_rate {
                parts.push(format!("{} fps", (fps * 100.0).round() / 100.0));
            }
            parts.extend(info.video_codec.as_ref().map(|codec| match info.hardware {
                Some(api) => format!("{codec} ({api})"),
                None => codec.clone(),
            }));
            parts.extend(info.audio_codec.clone());
            parts.join(" · ")
        });

        let play_icon = if self.player.is_playing() {
            "pause"
        } else if duration.is_some_and(|d| position >= d - 0.05) {
            "replay"
        } else {
            "play"
        };
        let volume_icon = if self.player.is_muted() || self.player.volume() == 0.0 {
            "mute"
        } else {
            "volume"
        };
        let has_audio = info.as_ref().is_some_and(|i| i.has_audio);
        let tracks = info.as_ref().filter(|info| Self::has_track_choice(info));
        let subtitles_on = info.as_ref().is_some_and(|i| i.subtitle_track.is_some());
        let looping = self.player.is_looping();
        let mouse_in_window = self.mouse_in_window;
        let reveal = |el: gpui::Div| {
            if pinned {
                el
            } else if !mouse_in_window {
                el.opacity(0.)
            } else {
                el.opacity(0.)
                    .group_hover(group.clone(), |style| style.opacity(1.))
            }
        };

        let header = reveal(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .flex()
                .items_center()
                .gap_1()
                .pl_3()
                .pr_1()
                // Lines the buttons up with the traffic lights.
                .pt_2()
                .pb_4()
                .when(under_traffic_lights, |this| this.pl(TRAFFIC_LIGHTS_WIDTH))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(overlay::scrim(), 0.),
                    linear_color_stop(overlay::scrim_clear(), 1.),
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_baseline()
                .gap_2()
                .when(show_titles, |this| {
                    this.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(overlay::text())
                            .child(self.title.clone()),
                    )
                })
                .when_some(details.filter(|d| !d.is_empty()), |this, details| {
                    this.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(overlay::text_muted())
                            .child(details),
                    )
                }),
        )
        .when(!self.alone, |this| {
            this.child(
                IconButton::new("pop-out", "pop-out")
                    .overlay()
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(PlayerEvent::PopOut))),
            )
        })
        .child(
            IconButton::new("maximize", if expanded { "minimize" } else { "maximize" })
                .overlay()
                .on_click(cx.listener(|this, _, window, cx| this.toggle_expanded(window, cx))),
        )
        .child(
            IconButton::new("close", "close")
                .overlay()
                .on_click(cx.listener(|_, _, _, cx| cx.emit(PlayerEvent::Close))),
        );

        let controls = reveal(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .px_2()
                .pt_6()
                .pb_1()
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(overlay::scrim_clear(), 0.),
                    linear_color_stop(overlay::scrim(), 1.),
                )),
        )
        .when(!live, |this| {
            this.child(
                div().px_1().child(
                    Slider::new("seek", progress, self.seek_drag.clone())
                        .overlay()
                        .fill(theme.accent)
                        .on_change(cx.listener(Self::on_seek)),
                ),
            )
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_0p5()
                .child(
                    IconButton::new("play", play_icon)
                        .overlay()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.player.toggle();
                            cx.notify();
                        })),
                )
                .when(live, |this| {
                    this.child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_1()
                            .ml_1()
                            .px_1p5()
                            .rounded(px(4.))
                            .bg(theme.danger)
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(overlay::text())
                            .child("LIVE"),
                    )
                })
                .child(
                    div()
                        .flex_none()
                        .pl_1()
                        .text_xs()
                        .text_color(overlay::text())
                        .child(time_label),
                )
                .child(div().flex_1())
                .when_some(tracks, |this, info| {
                    let button = IconButton::new("tracks", "subtitles")
                        .overlay()
                        .when(subtitles_on, |this| this.color(theme.accent))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.tracks_open = !this.tracks_open;
                            cx.notify();
                        }));
                    this.child(
                        div()
                            .relative()
                            .child(button)
                            .when(self.tracks_open, |this| {
                                this.child(
                                    deferred(
                                        anchored()
                                            .position_mode(AnchoredPositionMode::Local)
                                            .anchor(Corner::BottomRight)
                                            .position(point(px(28.), px(-6.)))
                                            .snap_to_window_with_margin(px(8.))
                                            .child(self.render_tracks_menu(info, &theme, cx)),
                                    )
                                    .with_priority(2),
                                )
                                // Clicks anywhere else close the menu, and do
                                // nothing else.
                                .child(
                                    deferred(
                                        anchored().position(point(px(0.), px(0.))).child(
                                            div()
                                                .id("tracks-backdrop")
                                                .occlude()
                                                .w(window.viewport_size().width)
                                                .h(window.viewport_size().height)
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(close_tracks),
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Right,
                                                    cx.listener(close_tracks),
                                                ),
                                        ),
                                    )
                                    .with_priority(1),
                                )
                            }),
                    )
                })
                .when(!live, |this| {
                    this.child(
                        IconButton::new("loop", "loop")
                            .overlay()
                            .color(if looping {
                                theme.accent
                            } else {
                                overlay::text_muted()
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                let looping = !this.player.is_looping();
                                this.player.set_looping(looping);
                                cx.notify();
                            })),
                    )
                })
                .when(has_audio, |this| {
                    this.child(IconButton::new("mute", volume_icon).overlay().on_click(
                        cx.listener(|this, _, _, cx| {
                            let muted = !this.player.is_muted();
                            this.player.set_muted(muted);
                            cx.notify();
                        }),
                    ))
                    .child(
                        div().w(px(72.)).pr_1().child(
                            Slider::new(
                                "volume",
                                if self.player.is_muted() {
                                    0.0
                                } else {
                                    self.player.volume()
                                },
                                self.volume_drag.clone(),
                            )
                            .overlay()
                            .on_change(cx.listener(Self::on_volume)),
                        ),
                    )
                }),
        );

        let osd = self.osd.as_ref().and_then(|(osd, shown)| {
            let shown = shown.elapsed();
            let left = OSD_DURATION.checked_sub(shown)?;
            let fade = (left.as_secs_f32() / OSD_FADE.as_secs_f32()).min(1.0);
            let note = match osd {
                Osd::Text(text) => Panel::new()
                    .overlay()
                    .rounded_full()
                    .px_3()
                    .py_1()
                    .child(text.clone())
                    .into_any_element(),
                Osd::Reel(reel) => render_reel(reel, shown),
            };
            Some((note, fade))
        });
        if osd.is_some() {
            window.request_animation_frame();
        } else {
            self.osd = None;
        }
        let outlined = self.active && !self.alone && !self.maximized && mouse_in_window;

        div()
            .id("player")
            .group(group.clone())
            .relative()
            .size_full()
            .overflow_hidden()
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                if !this.active {
                    cx.emit(PlayerEvent::Activate);
                }
            }))
            .bg(if has_video || info.as_ref().is_some_and(|i| i.has_video) {
                black()
            } else {
                theme.tile
            })
            .child(
                div()
                    .id("surface")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(Self::on_surface_click))
                    .child(self.render_content(&theme, fit)),
            )
            .when(!self.cues.is_empty(), |this| {
                this.child(subtitles::overlay(
                    self.cues.clone(),
                    subtitles::Layout {
                        video: info.as_ref().and_then(|i| i.video_size),
                        cover,
                        // The seek bar, the buttons and the padding below them.
                        controls_height: px(if live { 32. } else { 50. }),
                        controls_pinned: pinned,
                        mouse_in_window,
                        font: theme.font.clone(),
                        style: subtitle_style,
                    },
                ))
            })
            .when(stalled, |this| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            Panel::new()
                                .overlay()
                                .rounded_full()
                                .px_3()
                                .py_1()
                                .text_xs()
                                .child("Buffering…"),
                        ),
                )
            })
            .when_some(osd, |this, (note, fade)| {
                this.child(
                    div()
                        .absolute()
                        .top(px(44.))
                        .left_0()
                        .w_full()
                        .flex()
                        .justify_center()
                        .opacity(fade)
                        .child(note),
                )
            })
            .when(outlined, |this| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .border_2()
                        .border_color(theme.accent.opacity(0.8)),
                )
            })
            // Clicks on the bars mustn't reach the surface underneath (which
            // toggles playback), but they can't `occlude()` either: an occluding
            // child stops its parent counting as hovered, which would hide the
            // bars as soon as the pointer moved onto them.
            .child(header.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
            .child(controls.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
    }
}

fn close_tracks(
    view: &mut PlayerView,
    _: &gpui::MouseDownEvent,
    _: &mut Window,
    cx: &mut Context<PlayerView>,
) {
    view.tracks_open = false;
    cx.stop_propagation();
    cx.notify();
}

/// A track's row: a check if it's the one picked, its name, and what it is
/// (in `muted`).
fn track_row(choice: &TrackChoice, checked: bool, muted: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .h(px(TRACK_ROW))
        .px_2()
        .rounded(px(5.))
        // A character rather than an icon, so it turns white with the rest
        // of the row in the menu.
        .child(
            div()
                .flex_none()
                .w(px(14.))
                .when(checked, |this| this.child("✓")),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(choice.label.clone()),
        )
        .when_some(choice.detail.clone(), |this, detail| {
            this.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .group_hover("track-row", |style| style.text_color(white().opacity(0.8)))
                    .child(detail),
            )
        })
}

fn track_heading_row(heading: &'static str, theme: &Theme) -> Div {
    div()
        .px_3()
        .pt_2()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.text_muted)
        .child(heading)
}

/// The reel `shown` after a track was picked: it rolls up a row, from the
/// one before in the middle to the one picked, which then has the one
/// before above it and the next below, faded.
fn render_reel(reel: &TrackReel, shown: Duration) -> AnyElement {
    let count = reel.choices.len();
    let t = (shown.as_secs_f32() / REEL_ROLL.as_secs_f32()).min(1.0);
    let rolled = 1.0 - (1.0 - t).powi(3);
    // Rows from the middle once rolled: the one two before is in view above
    // until it rolls out.
    let rows: &[isize] = if count > 1 { &[-2, -1, 0, 1] } else { &[0] };
    let rows = rows.iter().filter_map(|&row| {
        let distance = row as f32 + if count > 1 { 1.0 - rolled } else { 0.0 };
        let opacity = reel_opacity(distance, count);
        if opacity <= 0.0 {
            return None;
        }
        let at = (reel.at as isize + row).rem_euclid(count as isize) as usize;
        Some(
            track_row(&reel.choices[at], row == 0, overlay::text_muted())
                .absolute()
                .left_0()
                .right_0()
                .top(px((1.0 + distance) * TRACK_ROW))
                .opacity(opacity),
        )
    });
    Panel::new()
        .overlay()
        .w(px(240.))
        .p_1()
        .child(
            div()
                .relative()
                .h(px(3.0 * TRACK_ROW))
                .overflow_hidden()
                .children(rows),
        )
        .into_any_element()
}

/// How bright a row on the reel is `distance` rows from the middle (below
/// is positive): full there, faded a row away, gone half a row further.
/// With two tracks the one above is the one below, so only that shows.
fn reel_opacity(distance: f32, count: usize) -> f32 {
    let neighbor = match count {
        0 | 1 => 0.0,
        2 if distance < 0.0 => 0.0,
        _ => REEL_FADED,
    };
    let away = distance.abs();
    if away <= 1.0 {
        1.0 + (neighbor - 1.0) * away
    } else {
        neighbor * (1.0 - (away - 1.0) * 2.0).max(0.0)
    }
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::{REEL_FADED, format_time, reel_opacity};

    #[test]
    fn fades_the_reel_away_from_the_middle() {
        let opacity = |distance, count| (reel_opacity(distance, count) * 100.0).round() / 100.0;
        assert_eq!(opacity(0.0, 5), 1.0);
        assert_eq!(opacity(-1.0, 5), REEL_FADED);
        assert_eq!(opacity(1.0, 5), REEL_FADED);
        assert_eq!(opacity(0.5, 5), 0.7);
        assert_eq!(opacity(1.5, 5), 0.0);
        assert_eq!(opacity(-2.0, 5), 0.0);
        // Two tracks: the other one shows below only.
        assert_eq!(opacity(1.0, 2), REEL_FADED);
        assert_eq!(opacity(-1.0, 2), 0.0);
        assert_eq!(opacity(0.0, 1), 1.0);
        assert_eq!(opacity(1.0, 1), 0.0);
    }

    #[test]
    fn formats_times() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(59.9), "0:59");
        assert_eq!(format_time(61.0), "1:01");
        assert_eq!(format_time(3725.0), "1:02:05");
        assert_eq!(format_time(-3.0), "0:00");
    }
}
