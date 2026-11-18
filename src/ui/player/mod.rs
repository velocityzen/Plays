//! A player's tile: the picture (or cover art) and subtitles, and over
//! them a header with the file's name, the controls, and notes on what
//! shortcuts did. Each part is in its own file; `render` puts them together.

mod commands;
mod content;
mod controls;
mod graveyard;
mod header;
mod osd;
mod tracks;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, Div, EventEmitter, MouseButton, ObjectFit, SharedString, Window, black,
    div,
};

pub use graveyard::Graveyard;

use super::components::Panel;
use super::subtitles;
use super::theme::Theme;
use super::theme::motion::LEVEL_FALLOFF;
use crate::media::picture::PictureId;
use crate::media::subtitles::CueContent;
use crate::media::{Mixer, Picture, Player, PlayerOptions, Source};
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
    /// Shortcuts go to this tile: the one last clicked.
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
        let live = info.as_ref().is_some_and(|i| i.live);
        // A stream that has started and then stalls; opening has its own message.
        let stalled = self.player.is_buffering()
            && self.player.is_network()
            && (has_video || info.as_ref().is_some_and(|i| !i.has_video));
        let mouse_in_window = self.mouse_in_window;
        let reveal = Reveal {
            pinned,
            mouse_in_window,
            group: format!("tile-{}", cx.entity_id()).into(),
        };

        let header = self.render_header(&reveal, info.as_ref(), show_titles, window, cx);
        let controls = self.render_controls(&reveal, info.as_ref(), &theme, cx);
        let osd = self.render_osd(window);

        div()
            .id("player")
            .group(reveal.group.clone())
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
                        controls_height: controls::height(live),
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
                        .child(Panel::note("Buffering…").text_xs()),
                )
            })
            .children(osd)
            // Clicks on the bars mustn't reach the surface underneath (which
            // toggles playback), but they can't `occlude()` either: an occluding
            // child stops its parent counting as hovered, which would hide the
            // bars as soon as the pointer moved onto them.
            .child(header.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
            .child(controls.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
    }
}
