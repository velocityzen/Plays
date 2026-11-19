//! A player's tile: the picture (or cover art) and subtitles, and over
//! them a header with the file's name, the controls, and notes on what
//! shortcuts did. Each part is in its own file; `render` puts them together.

mod adjust;
mod cast;
mod commands;
mod console;
mod content;
mod controls;
mod graveyard;
mod header;
mod osd;
mod pip;
mod screenshot;
mod sync;
mod tracks;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    Bounds, ClickEvent, Context, CursorStyle, Div, EventEmitter, MouseButton, Pixels, SharedString,
    Task, Window, black, div,
};

pub use graveyard::Graveyard;

use super::components::Panel;
use super::framing::{Adjustments, Framing};
use super::subtitles;
use super::theme::Theme;
use super::theme::motion::LEVEL_FALLOFF;
use crate::media::decoder::MediaInfo;
use crate::media::picture::PictureId;
use crate::media::subtitles::CueContent;
use crate::media::{Mixer, Neighbors, Picture, Player, PlayerOptions, Source};
use crate::settings::{ControlsVisibility, Settings, VideoFit};
use osd::Osd;

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
    /// The files before and after this one in its folder, for the previous
    /// and next buttons; `None` while the folder is being listed.
    neighbors: Option<Neighbors>,
    /// Lists the folder (`find_neighbors`).
    finding_neighbors: Task<()>,
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
    /// How the picture is turned, reshaped and cropped.
    adjustments: Adjustments,
    /// Where the picture was last drawn in the window, for Picture in
    /// Picture to open from.
    picture_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Picture in Picture, while it is open or opening.
    pip: Option<pip::PipSession>,
    /// Shortcuts go to this tile: the one last clicked.
    pub active: bool,
    pub maximized: bool,
    /// The only tile in its window: there is nowhere to expand it to, and
    /// popping it out would leave the window empty.
    pub alone: bool,
    /// In its window's top-left corner, where the macOS traffic lights are.
    pub top_left: bool,
    /// The controls show only while the mouse is over the tile, which needs
    /// the mouse in the window, and not resting (`mouse_idle`) unless it is
    /// on the header or controls.
    pub mouse_in_window: bool,
    pub mouse_idle: bool,
    over_header: bool,
    over_controls: bool,
    /// The console, while it shows: what keeps it up to date.
    console: Option<Task<()>>,
    /// The device it plays on instead, while it casts.
    cast: Option<cast::Cast>,
    /// The menu of devices to cast to is open.
    cast_open: bool,
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
            neighbors: None,
            finding_neighbors: Task::ready(()),
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
            adjustments: Adjustments::default(),
            picture_bounds: Rc::default(),
            pip: None,
            active: false,
            maximized: false,
            alone: true,
            top_left: false,
            mouse_in_window: true,
            mouse_idle: false,
            over_header: false,
            over_controls: false,
            console: None,
            cast: None,
            cast_open: false,
        }
    }

    /// Find the files before and after this one in its folder, in the
    /// background: the folder may be on a network drive.
    pub fn find_neighbors(&mut self, cx: &mut Context<Self>) {
        self.neighbors = None;
        let source = self.player.source().clone();
        let listing = cx.background_spawn(async move { source.neighbors() });
        self.finding_neighbors = cx.spawn(async move |this, cx| {
            let neighbors = listing.await;
            this.update(cx, |this, cx| {
                this.neighbors = Some(neighbors);
                cx.notify();
            })
            .ok();
        });
    }

    /// Play the media file before (`-1`) or after (`1`) this one in its
    /// folder instead, at the same volume, muted and looping as this one is,
    /// and playing if this one is (or files start playing). Without one,
    /// say so, as only a shortcut gets here then.
    pub fn open_neighbor(&mut self, direction: i32, cx: &mut Context<Self>) {
        let Some(neighbors) = &self.neighbors else {
            self.note("Still looking through its folder", cx);
            return;
        };
        let neighbor = if direction < 0 {
            neighbors.previous.clone()
        } else {
            neighbors.next.clone()
        };
        let Some(path) = neighbor else {
            let side = if direction < 0 { "previous" } else { "next" };
            self.note(format!("No {side} file in its folder"), cx);
            return;
        };
        let source = Source::File(path);
        log::info!("opening {source}");
        // A cast goes on with the new file, which waits here meanwhile.
        let casting = self.casting().cloned();
        let playing = self.is_playing();
        self.cast = None;
        let settings = cx.global::<Settings>();
        let autoplay = playing || settings.autoplay;
        let options = PlayerOptions {
            autoplay: autoplay && casting.is_none(),
            looping: self.player.is_looping(),
            volume: self.player.volume(),
            max_short_side: settings.max_resolution.short_side(),
            hardware_decoding: settings.hardware_decoding,
        };
        let muted = self.player.is_muted();
        self.release_pictures();
        self.title = source.name().into();
        self.player = Player::open(source, self.player.mixer(), options);
        self.find_neighbors(cx);
        self.player.set_muted(muted);
        self.drawn = None;
        self.scrub = None;
        self.level = 0.0;
        self.tracks_open = false;
        self.hidden_subtitles = None;
        self.adjustments = Adjustments::default();
        self.osd = None;
        if let Some(device) = casting {
            self.start_cast(device, 0.0, autoplay, cx);
        }
        cx.notify();
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    /// How the video shows, once its size is known: turned as the file
    /// says, then as adjusted here.
    pub fn framing(&self) -> Option<Framing> {
        self.framing_for(self.player.info().as_ref())
    }

    fn framing_for(&self, info: Option<&MediaInfo>) -> Option<Framing> {
        let info = info?;
        let size = info
            .video_size
            .or_else(|| self.player.frame().map(|frame| frame.size()))?;
        Framing::new(size, info.orientation, &self.adjustments)
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
        // Picture in Picture's layer is in the old window.
        self.close_picture_in_picture();
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
        self.close_picture_in_picture();
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
        // A single click only makes the tile the one shortcuts go to (on
        // mouse down); a double click expands it.
        if event.click_count() == 2 {
            self.toggle_expanded(window, cx);
            cx.notify();
        }
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

    /// Whether the mouse is on the header or the controls, which keeps them
    /// showing while it rests there.
    pub fn over_bars(&self) -> bool {
        self.over_header || self.over_controls
    }
}

/// Whether the header and controls show: always while pinned, otherwise
/// while the mouse is over the tile.
struct Reveal {
    pinned: bool,
    mouse_in_window: bool,
    /// The tile's hover group.
    group: SharedString,
}

impl Reveal {
    fn apply(&self, el: Div) -> Div {
        if self.pinned {
            el
        } else if !self.mouse_in_window {
            el.opacity(0.)
        } else {
            el.opacity(0.)
                .group_hover(self.group.clone(), |style| style.opacity(1.))
        }
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
            self.level * LEVEL_FALLOFF + level * (1. - LEVEL_FALLOFF)
        };
        if tick.animate || self.level > 0.005 {
            window.request_animation_frame();
        }

        let theme = Theme::get(window, cx);
        let settings = cx.global::<Settings>();
        let cover = settings.video_fit == VideoFit::Cover;
        let subtitle_style = settings.subtitles.clone();
        let show_titles = settings.show_titles;
        let info = self.player.info();
        let framing = self.framing_for(info.as_ref());
        let has_video = self.player.frame().is_some();
        let dragging = self.seek_drag.get() || self.volume_drag.get();
        let casting = self.cast.is_some();
        let pinned = settings.controls == ControlsVisibility::Always
            || dragging
            || self.tracks_open
            || self.cast_open
            || !self.is_playing()
            || self.player.error().is_some();
        // A resting mouse counts as gone, unless it rests on the bars; and
        // the pointer hides over the picture until it moves.
        let mouse_in_window = self.mouse_in_window && (!self.mouse_idle || self.over_bars());
        let hide_pointer = self.mouse_in_window && self.mouse_idle && !self.over_bars() && !pinned;
        let live = info.as_ref().is_some_and(|i| i.live);
        // A stream that has started and then stalls; opening has its own message.
        let stalled = !casting
            && self.player.is_buffering()
            && self.player.is_network()
            && (has_video || info.as_ref().is_some_and(|i| !i.has_video));
        let reveal = Reveal {
            pinned,
            mouse_in_window,
            group: format!("tile-{}", cx.entity_id()).into(),
        };

        let header = self.render_header(&reveal, info.as_ref(), show_titles, window, cx);
        let controls = self.render_controls(&reveal, info.as_ref(), &theme, cx);
        let osd = self.render_osd(window);
        let console = self.render_console();

        div()
            .id("player")
            .group(reveal.group.clone())
            .when(hide_pointer, |this| this.cursor(CursorStyle::None))
            .relative()
            .size_full()
            .overflow_hidden()
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                if !this.active {
                    cx.emit(PlayerEvent::Activate);
                }
            }))
            .bg(
                if !casting && (has_video || info.as_ref().is_some_and(|i| i.has_video)) {
                    black()
                } else {
                    theme.tile
                },
            )
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
                    .child(match self.render_cast(&theme, cx) {
                        Some(cast) => cast,
                        None => self.render_content(&theme, framing, cover),
                    }),
            )
            // In Picture in Picture, the subtitles are in its window.
            .when(
                !self.cues.is_empty() && !self.in_picture_in_picture() && !casting,
                |this| {
                    this.child(subtitles::overlay(
                        self.cues.clone(),
                        subtitles::Layout {
                            framing,
                            cover,
                            controls_height: controls::height(live),
                            controls_pinned: pinned,
                            mouse_in_window,
                            font: theme.font.clone(),
                            style: subtitle_style,
                        },
                    ))
                },
            )
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
                        .child(Panel::note("Buffering…").text_xs()),
                )
            })
            .children(console)
            .children(osd)
            // Clicks on the bars mustn't reach the surface underneath (which
            // toggles playback), but they can't `occlude()` either: an occluding
            // child stops its parent counting as hovered, which would hide the
            // bars as soon as the pointer moved onto them.
            .child(
                header
                    .id("header")
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.over_header = *hovered;
                        cx.notify();
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            )
            .child(
                controls
                    .id("controls")
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.over_controls = *hovered;
                        cx.notify();
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()),
            )
    }
}
