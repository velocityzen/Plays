//! A player's block: the picture (or cover art) and subtitles, and over
//! them a header with the file's name, the controls, and notes on what
//! shortcuts did. Each part is in its own file; `render` puts them together.

mod adjust;
mod cast;
mod commands;
mod console;
mod content;
mod controls;
mod drag;
mod failed;
mod graveyard;
mod header;
mod inspect;
mod osd;
mod pip;
mod playback;
mod playlist;
mod screenshot;
mod sound;
mod sync;
mod tracks;
mod visualizer;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use gpui::prelude::*;
use gpui::{
    App, Bounds, ClickEvent, Context, CursorStyle, Div, EventEmitter, Hsla, MouseButton,
    MouseMoveEvent, Pixels, Point, SharedString, Stateful, Task, Window, black, div,
};

pub use controls::format_time;
pub use drag::{DraggedBlock, move_window_once_dragged};
pub use graveyard::Graveyard;

use super::components::Panel;
use super::framing::{Adjustments, Framing};
use super::subtitles;
use super::theme::Theme;
use super::theme::motion::LEVEL_FALLOFF;
use crate::api::{self, Id, Invocation, Run};
use crate::media::decoder::MediaInfo;
use crate::media::picking::{Picked, Wanted};
use crate::media::picture::PictureId;
use crate::media::subtitles::CueContent;
use crate::media::{Mixer, Neighbors, Picture, PlayerOptions, Source};
use crate::progress::{self, Note};
use crate::recents;
use crate::settings::{
    AudioDevice, ControlsVisibility, OverVideo, Settings, SubtitleStyle, VideoFit,
};
use osd::Osd;
use playback::Playback;
use playlist::Playlist;

/// What a block tells its window. What its controls do, they run as
/// commands (`send`).
pub enum PlayerEvent {
    /// The mouse came over it, or it was clicked: shortcuts go to this
    /// block now.
    Activate,
    /// It was clicked: it's the block Go Back One Block comes back to.
    Pressed,
}

/// Where a block is in its window, and where the mouse is, as the window
/// last said (`PlayerView::set_layout`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockLayout {
    /// Shortcuts go to this block: the one the mouse was last over, or
    /// clicked, or the keys went to.
    pub active: bool,
    /// It was put in front from the keyboard, among other blocks, and the
    /// mouse hasn't moved since: a ring shows which it is.
    pub focus_ring: bool,
    /// It fills the grid, the other blocks hidden.
    pub maximized: bool,
    /// The only block in its window: there is nowhere to expand it to, and
    /// popping it out would leave the window empty.
    pub alone: bool,
    /// In its window's top-left corner, where the macOS traffic lights are.
    pub top_left: bool,
    /// The controls show only while the mouse is over the block, which needs
    /// the mouse in the window, and not resting (`mouse_idle`) unless it is
    /// on the header or controls.
    pub mouse_in_window: bool,
    pub mouse_idle: bool,
}

pub struct PlayerView {
    /// What the API knows it by, from window to window.
    id: Id,
    /// What it's called, besides its id, if it's been named.
    label: Option<String>,
    /// The player here, and the device it plays on instead while it casts.
    playback: Playback,
    title: SharedString,
    /// The files before and after this one in its folder, for the previous
    /// and next buttons; `None` while the folder is being listed.
    neighbors: Option<Neighbors>,
    /// Lists the folder (`find_neighbors`).
    finding_neighbors: Task<()>,
    /// The files opened with this one, to play one after another, as
    /// Settings → Playback can have them; for the previous and next
    /// buttons in place of its folder's.
    playlist: Option<Playlist>,
    graveyard: Graveyard,
    seek_drag: Rc<Cell<bool>>,
    volume_drag: Rc<Cell<bool>>,
    /// Position shown while the seek slider is being dragged.
    scrub: Option<f64>,
    level: f32,
    /// The frame handed to gpui in the last render (and so maybe in use on the GPU).
    drawn: Option<PictureId>,
    /// The subtitles handed to gpui in the last render.
    cues: Vec<Arc<CueContent>>,
    /// The audio and subtitle track menu is open.
    tracks_open: bool,
    /// The panels it shows over its video.
    over_video: OverVideo,
    /// A note on what a shortcut did, and when it was shown.
    osd: Option<(Osd, Instant)>,
    /// The subtitles hidden with a shortcut, to show again.
    hidden_subtitles: Option<usize>,
    /// The tracks picked in its file, since it opened or before, as
    /// `progress` keeps them for it.
    picked: Picked,
    /// How the picture is turned, reshaped and cropped.
    adjustments: Adjustments,
    /// Where the picture was last drawn in the window, for Picture in
    /// Picture to open from.
    picture_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Picture in Picture, while it is open or opening.
    pip: Option<pip::PipSession>,
    /// Where it is in its window, as the window last said.
    layout: BlockLayout,
    over_header: bool,
    over_controls: bool,
    /// The console, while it shows: what keeps it up to date.
    console: Option<Task<()>>,
    /// The menu of devices to cast to is open.
    cast_open: bool,
    /// The device its window plays through instead of the app's output,
    /// which its menu checks.
    window_sound: Option<AudioDevice>,
    /// Where the picture was pressed, while that may yet move the window.
    window_press: Option<Point<Pixels>>,
    /// Drawing it panicked, while the file it had opened then (counted by
    /// `Playback::openings`) is open: it shows why, and only that.
    drawing_failed: Option<u64>,
    /// How many times the picture has moved the window.
    #[cfg(test)]
    pub window_moves: usize,
    /// What drawing it next panics with, for tests.
    #[cfg(test)]
    pub panic_while_drawing: Option<&'static str>,
}

impl EventEmitter<PlayerEvent> for PlayerView {}

/// What the API knows a block by.
impl PlayerView {
    pub fn id(&self) -> Id {
        self.id
    }

    /// Run `command` on this block, given `args` (null for none), once the
    /// event is handled: what its controls do.
    pub(super) fn send(&self, command: &str, args: serde_json::Value, cx: &mut gpui::App) {
        let invocation = Invocation::new(command)
            .with_args(args)
            .on_block(self.id.to_string());
        api::dispatch(invocation, cx);
    }

    /// Run on this block what `run` runs on the block in front, as a menu
    /// item does.
    pub(super) fn send_run(&self, Run(invocation): Run, cx: &mut gpui::App) {
        self.send(&invocation.command, invocation.args, cx);
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Label the block, or take its label away: its header shows the label
    /// in place of the file's name.
    pub fn set_label(&mut self, label: Option<String>, cx: &mut Context<Self>) {
        self.label = label;
        cx.notify();
    }

    /// What its header calls it: its label, else its file's name.
    pub fn header_title(&self) -> SharedString {
        self.label
            .clone()
            .map_or_else(|| self.title.clone(), SharedString::from)
    }
}

/// The tracks to play as `source` opens: those picked in it before, if
/// Settings say to resume files, else in the languages Settings prefer.
fn wanted(source: &Source, cx: &App) -> Wanted {
    let picked = match source {
        Source::File(path) => progress::picked(path, cx),
        Source::Url(_) => Picked::default(),
    };
    let languages = &cx.global::<Settings>().languages;
    Wanted {
        picked,
        audio_languages: languages.audio.clone(),
        subtitle_languages: languages.subtitles.clone(),
    }
}

impl PlayerView {
    pub fn new(source: Source, mixer: Option<Arc<Mixer>>, graveyard: Graveyard, cx: &App) -> Self {
        let title = source.name().into();
        let settings = cx.global::<Settings>();
        let wanted = wanted(&source, cx);
        let picked = wanted.picked;
        let playback = Playback::open(
            source,
            mixer,
            PlayerOptions {
                autoplay: settings.autoplay,
                looping: settings.loop_by_default,
                volume: settings.default_volume,
                max_short_side: settings.max_resolution.short_side(),
                hardware_decoding: settings.hardware_decoding,
                wanted,
            },
        );
        Self {
            id: Id::next(),
            label: None,
            playback,
            title,
            neighbors: None,
            finding_neighbors: Task::ready(()),
            playlist: None,
            graveyard,
            seek_drag: Rc::default(),
            volume_drag: Rc::default(),
            scrub: None,
            level: 0.0,
            drawn: None,
            cues: Vec::new(),
            tracks_open: false,
            over_video: settings.over_video,
            osd: None,
            hidden_subtitles: None,
            picked,
            adjustments: Adjustments::default(),
            picture_bounds: Rc::default(),
            pip: None,
            layout: BlockLayout {
                active: false,
                focus_ring: false,
                maximized: false,
                alone: true,
                top_left: false,
                mouse_in_window: true,
                mouse_idle: false,
            },
            over_header: false,
            over_controls: false,
            console: None,
            cast_open: false,
            window_sound: None,
            window_press: None,
            drawing_failed: None,
            #[cfg(test)]
            window_moves: 0,
            #[cfg(test)]
            panic_while_drawing: None,
        }
    }

    /// Find the files before and after this one in its folder, in the
    /// background: the folder may be on a network drive.
    pub fn find_neighbors(&mut self, cx: &mut Context<Self>) {
        self.neighbors = None;
        let source = self.playback.local().source().clone();
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

    /// Its folder has been looked through, for the files before and after.
    #[cfg(test)]
    pub fn knows_neighbors(&self) -> bool {
        self.neighbors.is_some()
    }

    /// The sound's level, from 0 to 1, as its speaker shows it.
    #[cfg(test)]
    pub fn level(&self) -> f32 {
        self.level
    }

    /// Its speaker shows `level`, falling from there, as if it had just
    /// played that loud: tests have no speaker to measure it at.
    #[cfg(test)]
    pub fn set_level(&mut self, level: f32) {
        self.level = level;
    }

    /// Play `sources`, the first of which it plays, one after another;
    /// none for fewer than two.
    pub fn set_playlist(&mut self, sources: Vec<Source>, cx: &mut Context<Self>) {
        self.playlist = Playlist::new(sources);
        cx.notify();
    }

    /// Where what it plays is in its playlist, as its header says: "2 of
    /// 5"; none without one.
    pub fn playlist_counter(&self) -> Option<String> {
        self.playlist.as_ref().map(Playlist::counter)
    }

    /// There's a file before (`-1`) or after (`1`) this one: in its
    /// playlist, if it has one, else in its folder, once looked through.
    fn has_beside(&self, direction: i32) -> bool {
        if let Some(playlist) = &self.playlist {
            return playlist.beside(direction).is_some();
        }
        self.neighbors.as_ref().is_some_and(|neighbors| {
            if direction < 0 {
                neighbors.previous.is_some()
            } else {
                neighbors.next.is_some()
            }
        })
    }

    /// Play the media file before (`-1`) or after (`1`) this one instead:
    /// in its playlist, if it has one, else in its folder (`play_instead`).
    /// Without one, say so, as only a shortcut gets here then.
    pub fn open_neighbor(&mut self, direction: i32, cx: &mut Context<Self>) {
        let side = if direction < 0 { "previous" } else { "next" };
        if let Some(playlist) = &mut self.playlist {
            match playlist.step(direction) {
                Some(source) => {
                    let playing = self.is_playing();
                    self.switch_to(source, playing, cx);
                }
                None => self.note(format!("No {side} file in its playlist"), cx),
            }
            return;
        }
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
            self.note(format!("No {side} file in its folder"), cx);
            return;
        };
        self.play_instead(path, cx);
    }

    /// Play the next file in its playlist once this one has played to its
    /// end, playing.
    fn follow_playlist(&mut self, cx: &mut Context<Self>) {
        if !self.playback.played_out() {
            return;
        }
        if let Some(next) = self.playlist.as_mut().and_then(|playlist| playlist.step(1)) {
            self.switch_to(next, true, cx);
        }
    }

    /// The media files in its file's folder, by name, once it has been
    /// looked through; none for a stream.
    pub fn folder_files(&self) -> Option<&[PathBuf]> {
        self.neighbors
            .as_ref()
            .map(|neighbors| neighbors.all.as_slice())
    }

    /// Its file's path; none for a stream.
    pub fn file(&self) -> Option<&Path> {
        match self.playback.local().source() {
            Source::File(path) => Some(path),
            Source::Url(_) => None,
        }
    }

    /// Play `path` instead, at the same volume, muted and looping as this
    /// file is, and playing if it is (or files start playing). In its
    /// playlist, it plays on from there; not, it ends the playlist.
    pub fn play_instead(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let source = Source::File(path);
        if self
            .playlist
            .as_mut()
            .is_some_and(|playlist| !playlist.go_to(&source))
        {
            self.playlist = None;
        }
        let playing = self.is_playing();
        self.switch_to(source, playing, cx);
    }

    /// Play `source` instead, at the same volume, muted and looping as this
    /// file is, and playing if `playing` (or files start playing).
    fn switch_to(&mut self, source: Source, playing: bool, cx: &mut Context<Self>) {
        if let Source::File(path) = &source {
            recents::note_file(path, cx);
        }
        log::info!("opening {source}");
        // The file it played is left where it was, to resume.
        progress::note(self.progress(), true, cx);
        // A cast goes on with the new file, which waits here meanwhile.
        let casting = self.casting().cloned();
        let settings = cx.global::<Settings>();
        let autoplay = playing || settings.autoplay;
        let player = self.playback.local();
        let wanted = wanted(&source, cx);
        self.picked = wanted.picked;
        let options = PlayerOptions {
            autoplay: autoplay && casting.is_none(),
            looping: player.is_looping(),
            volume: player.volume(),
            max_short_side: settings.max_resolution.short_side(),
            hardware_decoding: settings.hardware_decoding,
            wanted,
        };
        let muted = player.is_muted();
        self.release_pictures();
        self.title = source.name().into();
        self.playback.reopen(source, options);
        self.resume(cx);
        self.find_neighbors(cx);
        self.playback.set_muted(muted);
        self.drawn = None;
        self.scrub = None;
        self.level = 0.0;
        self.tracks_open = false;
        self.hidden_subtitles = None;
        self.adjustments = Adjustments::default();
        self.osd = None;
        if let Some(device) = casting {
            self.cast_once_known(device, autoplay, cx);
        }
        cx.notify();
    }

    /// Take where the block is in its window, and where the mouse is, as
    /// the window says; it redraws if that changed.
    pub fn set_layout(&mut self, layout: BlockLayout, cx: &mut Context<Self>) {
        if self.layout != layout {
            self.layout = layout;
            cx.notify();
        }
    }

    /// Where the block is in its window, as the window last said.
    #[cfg(test)]
    pub fn layout(&self) -> BlockLayout {
        self.layout
    }

    /// How the video shows, once its size is known: turned as the file
    /// says, then as adjusted here.
    pub fn framing(&self) -> Option<Framing> {
        self.framing_for(self.playback.local().info().as_deref())
    }

    fn framing_for(&self, info: Option<&MediaInfo>) -> Option<Framing> {
        let info = info?;
        let size = info
            .video_size
            .or_else(|| self.playback.local().frame().map(|frame| frame.size()))?;
        Framing::new(size, info.orientation, &self.adjustments)
    }

    /// How pictures look changed (gamma). Surfaces take it as they are
    /// drawn and new frames as they are converted, so only a paused video
    /// showing a converted image needs its frame decoded again.
    pub fn picture_settings_changed(&mut self) {
        self.playback.refresh_picture();
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
    /// window isn't drawing, for example when it is minimized; and plays
    /// the next file in its playlist once one has played to its end.
    pub fn background_tick(&mut self, cx: &mut Context<Self>) {
        let was_playing = self.playback.local().is_playing();
        let tick = self.playback.tick();
        let advanced = tick.retired.is_some();
        if let Some(picture) = tick.retired {
            self.retire(picture);
        }
        if advanced || was_playing != self.playback.local().is_playing() {
            cx.notify();
        }
        self.follow_playlist(cx);
    }

    /// Where its file is, of how long, once it has opened, and the tracks
    /// picked in it: to resume it there with them. Only files, not
    /// streams.
    pub fn progress(&self) -> Option<Note> {
        let Source::File(path) = self.playback.local().source() else {
            return None;
        };
        Some(Note {
            file: path.clone(),
            position: self.position(),
            duration: self.duration()?,
            picked: self.picked,
        })
    }

    /// Start its file where it was left, if Settings say to and it played
    /// lately (`progress::KEEP_DAYS`).
    pub fn resume(&mut self, cx: &mut Context<Self>) {
        let Source::File(path) = self.playback.local().source() else {
            return;
        };
        let Some(position) = progress::recall(path, cx) else {
            return;
        };
        self.playback.seek(position);
        self.note_change(
            format!("Resumed at {}", controls::format_time(position)),
            cx,
        );
    }

    /// Play into `mixer` from now on, its window's, which plays through
    /// `device` instead of the app's output, if any.
    pub fn play_into(&mut self, mixer: Option<Arc<Mixer>>, device: Option<AudioDevice>) {
        self.playback.set_mixer(mixer);
        self.window_sound = device;
    }

    /// Move this block to another window, whose frames are released through
    /// `graveyard`.
    pub fn move_to_window(&mut self, graveyard: Graveyard) {
        // Picture in Picture's layer is in the old window.
        self.close_picture_in_picture();
        // Pictures drawn so far are in the old window's texture atlas, so the
        // old window releases them; the new one uploads its own copies.
        let player = self.playback.local();
        let cover = player.cover().map(Picture::Image);
        for picture in player.frame().into_iter().chain(cover) {
            self.graveyard.bury(picture);
        }
        for cue in std::mem::take(&mut self.cues) {
            self.bury_cue(&cue);
        }
        self.graveyard = graveyard;
    }

    /// Hand every frame this block drew to the graveyard; call before dropping it.
    pub fn release_pictures(&mut self) {
        self.close_picture_in_picture();
        for picture in self.playback.take_pictures() {
            self.graveyard.bury(picture);
        }
        for cue in std::mem::take(&mut self.cues) {
            self.bury_cue(&cue);
        }
    }

    fn on_surface_click(&mut self, event: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        // A single click only makes the block the one shortcuts go to (on
        // mouse down); a double click expands it.
        if event.click_count() == 2 {
            self.send("block.zoom", serde_json::Value::Null, cx);
        }
    }

    /// Whether the mouse is on the header or the controls, which keeps them
    /// showing while it rests there.
    pub fn over_bars(&self) -> bool {
        self.over_header || self.over_controls
    }
}

/// Whether the header and controls show: always while pinned, otherwise
/// while the mouse is over the block.
struct Reveal {
    pinned: bool,
    mouse_in_window: bool,
    /// The block's hover group.
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
        self.render_unless_failing(window, cx)
    }
}

impl PlayerView {
    /// The block as it's drawn: what it plays, and the bars and panels
    /// over it.
    fn render_block(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        #[cfg(test)]
        if let Some(why) = self.panic_while_drawing {
            panic!("{why}");
        }
        self.take_tick(window);

        let theme = Theme::get(window, cx);
        let settings = cx.global::<Settings>();
        let cover = settings.video_fit == VideoFit::Cover;
        let subtitle_style = settings.subtitles.clone();
        let player = self.playback.local();
        let info = player.info();
        let framing = self.framing_for(info.as_deref());
        let has_video = player.frame().is_some();
        let casting = self.casts_picture();
        let pinned = self.bars_pinned(settings.controls == ControlsVisibility::Always);
        // A resting mouse counts as gone, unless it rests on the bars; and
        // the pointer hides over the picture until it moves.
        let layout = self.layout;
        let mouse_in_window = layout.mouse_in_window && (!layout.mouse_idle || self.over_bars());
        let hide_pointer =
            layout.mouse_in_window && layout.mouse_idle && !self.over_bars() && !pinned;
        // A stream that has started and then stalls; opening has its own message.
        let stalled = !casting
            && player.is_buffering()
            && player.is_network()
            && (has_video || info.as_ref().is_some_and(|i| !i.has_video));
        let reveal = Reveal {
            pinned,
            mouse_in_window,
            group: format!("block-{}", cx.entity_id()).into(),
        };

        let bars = self.render_bars(&reveal, info.as_deref(), &theme, window, cx);
        let osd = self.render_osd(window);
        let console = self.render_console();

        let surface = self.render_surface(window, cx);

        div()
            .id("player")
            .group(reveal.group.clone())
            .when(hide_pointer, |this| this.cursor(CursorStyle::None))
            .relative()
            .size_full()
            .overflow_hidden()
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                if !this.layout.active {
                    cx.emit(PlayerEvent::Activate);
                }
                cx.emit(PlayerEvent::Pressed);
            }))
            // Shortcuts go to the block the mouse is over, as its controls
            // show there; not while it drags something of another block's.
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if !this.layout.active && event.pressed_button.is_none() {
                    cx.emit(PlayerEvent::Activate);
                }
            }))
            .bg(
                if !casting && (has_video || info.as_ref().is_some_and(|i| i.has_video)) {
                    black()
                } else {
                    theme.block
                },
            )
            .child(surface.child(match self.render_cast(&theme, cx) {
                Some(cast) => cast,
                None => self.render_content(&theme, framing, cover, cx),
            }))
            // In Picture in Picture, the subtitles are in its window.
            .when(
                !self.cues.is_empty() && !self.in_picture_in_picture() && !casting,
                |this| {
                    this.child(self.render_subtitles(
                        &reveal,
                        framing,
                        cover,
                        self.subtitle_clearance(),
                        &theme,
                        subtitle_style,
                    ))
                },
            )
            .when(stalled, |this| this.child(render_buffering()))
            .children(console)
            .children(osd)
            .children(bars)
            .when(layout.focus_ring, |this| {
                this.child(render_focus_ring(theme.accent))
            })
    }
}

impl PlayerView {
    /// The header and the controls, those Settings has shown over the
    /// video: one hidden isn't drawn, so the mouse isn't over it, and no
    /// menu is open from it.
    fn render_bars(
        &mut self,
        reveal: &Reveal,
        info: Option<&MediaInfo>,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Stateful<Div>> {
        let (shown, show_titles) = (self.over_video, cx.global::<Settings>().show_titles);
        if !shown.top_bar {
            self.over_header = false;
        }
        if !shown.controls {
            self.over_controls = false;
            self.tracks_open = false;
            self.cast_open = false;
        }
        let header = shown
            .top_bar
            .then(|| self.render_header(reveal, info, show_titles, window, cx));
        let controls = shown
            .controls
            .then(|| self.render_controls(reveal, info, theme, window, cx));
        // Presses on the bars mustn't reach the surface underneath (which
        // expands the block and drags it), but they can't `occlude()`
        // either: an occluding child stops its parent counting as hovered,
        // which would hide the bars as soon as the pointer moved onto them.
        let header = header.map(|header| {
            header
                .id("header")
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    this.over_header = *hovered;
                    cx.notify();
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        });
        let controls = controls.map(|controls| {
            controls
                .id("controls")
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    this.over_controls = *hovered;
                    cx.notify();
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        });
        header.into_iter().chain(controls).collect()
    }

    /// Take what playback has for this frame: retire the picture it drew
    /// over, show the cues now, follow the sound's level, and ask for the
    /// next frame while anything moves.
    fn take_tick(&mut self, window: &mut Window) {
        let tick = self.playback.tick();
        if let Some(picture) = tick.retired {
            self.retire(picture);
        }
        self.drawn = self.playback.local().frame().map(|frame| frame.id());
        let cues = self.playback.local().subtitles();
        self.show_cues(cues);
        let level = self.playback.local().level();
        self.level = if level > self.level {
            level
        } else {
            self.level * LEVEL_FALLOFF + level * (1. - LEVEL_FALLOFF)
        };
        if tick.animate || self.level > 0.005 {
            window.request_animation_frame();
        }
    }

    /// Whether the header and controls show wherever the mouse is: `always`,
    /// as Settings says, or while something on them is in use, or while it
    /// isn't playing.
    fn bars_pinned(&self, always: bool) -> bool {
        let dragging = self.seek_drag.get() || self.volume_drag.get();
        always
            || dragging
            || self.tracks_open
            || self.cast_open
            || !self.is_playing()
            || self.playback.local().error().is_some()
    }

    /// What the picture shows on, filling the block: a double click on it
    /// expands the block, and dragging it drags the block.
    fn render_surface(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let surface = div()
            .id("surface")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(Self::on_surface_click));
        self.drag_by_picture(surface, window, cx)
    }

    /// How far up the block its subtitles keep clear of, for the controls
    /// while they show: none while it has them hidden.
    pub fn subtitle_clearance(&self) -> Pixels {
        let live = self.playback.local().info().is_some_and(|info| info.live);
        controls::clearance(self.over_video.controls, live)
    }

    /// The panels it shows over its video.
    pub fn over_video(&self) -> OverVideo {
        self.over_video
    }

    /// Show the panels `shown` has over its video, and hide the others.
    pub fn set_over_video(&mut self, shown: OverVideo, cx: &mut Context<Self>) {
        self.over_video = shown;
        cx.notify();
    }

    /// The cues showing, placed on the picture and kept above the controls
    /// while they show, `controls_height` up the block.
    fn render_subtitles(
        &self,
        reveal: &Reveal,
        framing: Option<Framing>,
        cover: bool,
        controls_height: Pixels,
        theme: &Theme,
        style: SubtitleStyle,
    ) -> impl IntoElement {
        subtitles::overlay(
            self.cues.clone(),
            subtitles::Layout {
                framing,
                cover,
                controls_height,
                controls_pinned: reveal.pinned,
                mouse_in_window: reveal.mouse_in_window,
                font: theme.font.clone(),
                style,
            },
        )
    }
}

/// A ring round the block the keys went to, over everything in it.
fn render_focus_ring(color: Hsla) -> Div {
    div()
        .debug_selector(|| "focus-ring".into())
        .absolute()
        .inset_0()
        .border_2()
        .border_color(color)
}

/// Says a stream is buffering, in the middle of the block.
fn render_buffering() -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(Panel::note("Buffering…").text_xs())
}
