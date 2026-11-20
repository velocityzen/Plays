//! Playing a tile in Picture in Picture (`ui::pip`). The window it came
//! from may not be drawing (gpui stops while it is covered or minimized),
//! so a pump of its own moves the player along and hands over each frame as
//! it is due, with the subtitles showing laid out for it; and what the
//! Picture in Picture window's buttons ask for comes back here.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc::unbounded;
use gpui::{Bounds, Context, Point, Task, Window};

use super::{PlayerEvent, PlayerView};
use crate::media::subtitles::CueContent;
use crate::media::{Orientation, Picture};
use crate::settings::{Settings, SubtitleStyle};
use crate::ui::framing::{Adjustments, Framing};
use crate::ui::overlay;
use crate::ui::pip::{self, Pip, PipEvent, Playback, Subtitles};
use crate::ui::theme::Theme;

/// How often the pump looks for the next frame.
const PUMP_EVERY: Duration = Duration::from_millis(4);
/// How long AVKit gets to be ready to open the window.
const START_WITHIN: Duration = Duration::from_secs(3);

/// A tile in Picture in Picture, from asking for it until it closes.
pub(super) struct PipSession {
    pip: Pip,
    /// Its window is open; the tile only says where the video is.
    pub(super) started: bool,
    asked_at: Instant,
    /// What the subtitles handed over were laid out for.
    laid_out: Option<Layout>,
    _pump: Task<()>,
}

/// The cues showing, how they look, and the size of the frames they go on.
#[derive(PartialEq)]
struct Layout {
    cues: Vec<*const CueContent>,
    style: SubtitleStyle,
    picture: (u32, u32),
    orientation: Orientation,
    size: (u32, u32),
}

impl PlayerView {
    /// Whether the video is in the Picture in Picture window.
    pub fn in_picture_in_picture(&self) -> bool {
        self.pip.as_ref().is_some_and(|session| session.started)
    }

    /// Open Picture in Picture for this tile, or close it.
    pub fn toggle_picture_in_picture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pip.take().is_some() {
            cx.notify();
            return;
        }
        if !pip::supported() {
            self.note(
                if cfg!(target_os = "macos") {
                    "Picture in Picture needs macOS 12 or later"
                } else {
                    "No Picture in Picture on this system"
                },
                cx,
            );
            return;
        }
        if self.casts_picture() {
            self.note("The picture plays on the device it casts to", cx);
            return;
        }
        let Some(frame) = self.player.frame() else {
            self.note("No picture for Picture in Picture", cx);
            return;
        };
        if frame.is_dolby_vision() {
            self.note("Picture in Picture can't show Dolby Vision", cx);
            return;
        }
        let bounds = self
            .picture_bounds
            .get()
            .unwrap_or_else(|| Bounds::new(Point::default(), window.viewport_size()));
        let (events, mut received) = unbounded();
        let pip = match Pip::new(window, bounds, self.player.is_live(), events) {
            Ok(pip) => pip,
            Err(err) => {
                log::warn!("Picture in Picture: {err:#}");
                self.note("Picture in Picture couldn't open", cx);
                return;
            }
        };
        let pump = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(PUMP_EVERY).await;
                let going = this.update_in(cx, |this, window, cx| {
                    while let Ok(event) = received.try_recv() {
                        this.on_pip_event(event, window, cx);
                    }
                    this.pump_pip(window, cx)
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        });
        self.pip = Some(PipSession {
            pip,
            started: false,
            asked_at: Instant::now(),
            laid_out: None,
            _pump: pump,
        });
        self.pump_pip(window, cx);
    }

    /// Close Picture in Picture, if it is open.
    pub(super) fn close_picture_in_picture(&mut self) {
        self.pip = None;
    }

    /// Move the player along, hand over the frame due now, and keep the
    /// window's buttons up to date. Whether to keep going.
    fn pump_pip(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.pip.is_none() {
            return false;
        }
        self.background_tick(cx);
        let cues = self.player.subtitles();
        self.show_cues(cues);
        let frame = self.player.frame();
        let orientation = self
            .framing()
            .map_or(Orientation::UPRIGHT, |framing| framing.orientation());
        // Where it plays, here or on the device its sound is cast to.
        let playback = Playback {
            playing: self.is_playing(),
            position: self.position(),
            duration: self.duration().filter(|_| !self.player.is_live()),
        };
        let Some(session) = &mut self.pip else {
            return false;
        };
        if session.started {
            session.pip.follow_window();
        }
        if let Some(frame) = &frame {
            self.lay_out_subtitles(frame, orientation, window, cx);
        }
        let Some(session) = &mut self.pip else {
            return false;
        };
        if let Some(frame) = &frame {
            session.pip.show(frame, orientation);
        }
        session.pip.update(playback);
        if !session.pip.start_when_possible() && session.asked_at.elapsed() > START_WITHIN {
            log::warn!("Picture in Picture never became possible");
            self.pip = None;
            self.note("Picture in Picture couldn't open", cx);
            return false;
        }
        true
    }

    /// Hand the subtitles showing over, laid out for frames like `frame`,
    /// when they or what they go on change.
    fn lay_out_subtitles(
        &mut self,
        frame: &Picture,
        orientation: Orientation,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &self.pip else {
            return;
        };
        let picture = frame.size();
        let shown = if orientation.sideways() {
            (picture.1, picture.0)
        } else {
            picture
        };
        let layout = Layout {
            cues: self.cues.iter().map(Arc::as_ptr).collect(),
            style: cx.global::<Settings>().subtitles.clone(),
            picture,
            orientation,
            size: session.pip.subtitle_size(shown),
        };
        if session.laid_out.as_ref() == Some(&layout) {
            return;
        }
        let subtitles = self.subtitles_for(&layout, window, cx);
        if let Some(session) = &mut self.pip {
            session.pip.set_subtitles(subtitles);
            session.laid_out = Some(layout);
        }
    }

    /// The cues showing, laid out as `layout` says; `None` if nothing shows.
    fn subtitles_for(
        &self,
        layout: &Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Subtitles> {
        if self.cues.is_empty() {
            return None;
        }
        let (width, height) = layout.size;
        let font = Theme::get(window, cx).font;
        let text = overlay::text_overlay(
            &self.cues,
            &layout.style,
            &font,
            (width as f32, height as f32),
            window,
        );
        // The window shows the picture turned, but not reshaped or cropped.
        let framing = Framing::new(layout.picture, layout.orientation, &Adjustments::default());
        let bitmaps: Vec<_> = self
            .cues
            .iter()
            .flat_map(|cue| &cue.bitmaps)
            .filter_map(|bitmap| {
                let (rect, sampling) = framing.as_ref()?.place(bitmap.rect);
                Some((bitmap.image.clone(), rect, sampling))
            })
            .collect();
        (!text.is_empty() || !bitmaps.is_empty()).then(|| Subtitles {
            size: layout.size,
            text,
            bitmaps,
            text_system: cx.text_system().clone(),
        })
    }

    pub(crate) fn on_pip_event(
        &mut self,
        event: PipEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::debug!("Picture in Picture: {event:?}");
        match event {
            // Its buttons drive the tile as its own do, and a cast with it.
            PipEvent::SetPlaying(true) => self.play(),
            PipEvent::SetPlaying(false) => self.pause(),
            PipEvent::Skip(seconds) => self.seek_by(seconds, cx),
            PipEvent::Started => {
                if let Some(session) = &mut self.pip {
                    session.started = true;
                    session.pip.set_layer_visible(false);
                }
            }
            PipEvent::Stopped => {
                let shown = self.pip.take().is_some_and(|session| session.started);
                // Closed from its own window while this one is full screen:
                // the video plays on here, in a Space of its own, out of
                // sight, so go back to it.
                if shown && window.is_fullscreen() {
                    bring_back(window, cx);
                }
            }
            PipEvent::Failed(why) => {
                log::warn!("Picture in Picture couldn't start: {why}");
                self.pip = None;
                self.note("Picture in Picture couldn't open", cx);
            }
            PipEvent::Restore => bring_back(window, cx),
        }
        cx.notify();
    }
}

/// Bring the tile's window forward, and with it its Space when it is full
/// screen in one. Plays is often not the app in front then, as Picture in
/// Picture floats over every app and Space, and a window of an app behind
/// can't come forward alone: the app comes first.
fn bring_back(window: &mut Window, cx: &mut Context<PlayerView>) {
    cx.activate(true);
    window.activate_window();
    cx.emit(PlayerEvent::Activate);
}
