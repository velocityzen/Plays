//! Playing a block in Picture in Picture (`ui::pip`). The window it came
//! from may not be drawing (gpui stops while it is covered or minimized),
//! so a pump of its own moves the player along and hands over each frame as
//! it is due, with the subtitles showing laid out for it; and what the
//! Picture in Picture window's buttons ask for comes back here.

use std::iter;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc::unbounded;
use futures::future::{self, Either};
use gpui::{Bounds, Context, Point, Task, Window};

use super::{PlayerEvent, PlayerView};
use crate::media::Picture;
use crate::media::subtitles::CueContent;
use crate::settings::{Settings, SubtitleStyle};
use crate::ui::framing::Framing;
use crate::ui::overlay;
use crate::ui::pip::{self, Pip, PipEvent, Playback, Subtitles};
use crate::ui::theme::Theme;

/// The least the pump waits for the next frame, due or not.
const PUMP_LEAST: Duration = Duration::from_millis(2);
/// How often it looks while playing with no frame decoded yet.
const PUMP_WAITING: Duration = Duration::from_millis(10);
/// How long it waits, at most, with no frame to come: paused, or with no
/// picture that moves. Enough to keep the window's place and its buttons
/// up to date; what its buttons ask for wakes it at once.
const PUMP_IDLE: Duration = Duration::from_millis(100);
/// How long AVKit gets to be ready to open the window.
const START_WITHIN: Duration = Duration::from_secs(3);

/// A block in Picture in Picture, from asking for it until it closes.
pub(super) struct PipSession {
    pip: Pip,
    /// Its window is open; the block only says where the video is.
    pub(super) started: bool,
    asked_at: Instant,
    /// What the subtitles handed over were laid out for.
    laid_out: Option<Layout>,
    _pump: Task<()>,
}

/// The cues showing, how they look, and the frames they go on: how big
/// they are decoded, how the block frames them, and their size.
struct Layout {
    /// Held, so none is let go of while laid out and another made where it
    /// was taken for it.
    cues: Vec<Arc<CueContent>>,
    style: SubtitleStyle,
    picture: (u32, u32),
    framing: Framing,
    size: (u32, u32),
}

impl PlayerView {
    /// Whether the video is in the Picture in Picture window.
    pub fn in_picture_in_picture(&self) -> bool {
        self.pip.as_ref().is_some_and(|session| session.started)
    }

    /// Open Picture in Picture for this block, or close it, as `turn` has
    /// it, given whether it's open, or opening.
    pub fn turn_picture_in_picture(
        &mut self,
        turn: impl FnOnce(bool) -> bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.pip.is_some();
        if turn(open) == open {
            return;
        }
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
        let Some(frame) = self.playback.local().frame() else {
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
        let pip = match Pip::new(window, bounds, self.playback.local().is_live(), events) {
            Ok(pip) => pip,
            Err(err) => {
                log::warn!("Picture in Picture: {err:#}");
                self.note("Picture in Picture couldn't open", cx);
                return;
            }
        };
        let pump = cx.spawn_in(window, async move |this, cx| {
            let mut wait = PUMP_LEAST;
            loop {
                let timer = cx.background_executor().timer(wait);
                // A button pressed in its window is seen to at once.
                let pressed = match future::select(timer, received.next()).await {
                    Either::Left(_) => None,
                    Either::Right((Some(event), _)) => Some(event),
                    Either::Right((None, _)) => break,
                };
                let next = this.update_in(cx, |this, window, cx| {
                    let more = iter::from_fn(|| received.try_recv().ok());
                    for event in pressed.into_iter().chain(more) {
                        this.on_pip_event(event, window, cx);
                    }
                    this.pump_pip(window, cx)
                });
                match next {
                    Ok(Some(next)) => wait = next,
                    _ => break,
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
    /// window's buttons up to date. How long to wait before the next time,
    /// if there is one.
    fn pump_pip(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Duration> {
        self.pip.as_ref()?;
        self.background_tick(cx);
        let cues = self.playback.local().subtitles();
        self.show_cues(cues);
        let frame = self.playback.local().frame();
        let framing = self.framing();
        // Where it plays, here or on the device its sound is cast to.
        let playing = self.is_playing();
        let playback = Playback {
            playing,
            position: self.position(),
            duration: self.duration().filter(|_| !self.playback.local().is_live()),
        };
        let session = self.pip.as_mut()?;
        if session.started {
            session.pip.follow_window();
        }
        // Shown as the block shows it: turned, cropped and shaped.
        let shown = frame.as_ref().zip(framing);
        if let Some((frame, framing)) = &shown {
            self.lay_out_subtitles(frame, framing, window, cx);
        }
        let session = self.pip.as_mut()?;
        if let Some((frame, framing)) = &shown {
            session.pip.show(frame, framing);
        }
        session.pip.update(playback);
        if !session.pip.start_when_possible() && session.asked_at.elapsed() > START_WITHIN {
            log::warn!("Picture in Picture never became possible");
            self.pip = None;
            self.note("Picture in Picture couldn't open", cx);
            return None;
        }
        Some(pump_wait(
            playing,
            self.playback.local().has_video(),
            self.playback.local().until_next_frame(),
        ))
    }

    /// Hand the subtitles showing over, laid out for frames like `frame`
    /// framed so, when they or what they go on change.
    fn lay_out_subtitles(
        &mut self,
        frame: &Picture,
        framing: &Framing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = &self.pip else {
            return;
        };
        let picture = frame.size();
        let shown = if framing.orientation().sideways() {
            (picture.1, picture.0)
        } else {
            picture
        };
        let style = &cx.global::<Settings>().subtitles;
        let size = session.pip.subtitle_size(shown, framing);
        // Looked at as it is, made anew only when something changed.
        let same = session.laid_out.as_ref().is_some_and(|laid| {
            laid.picture == picture
                && laid.framing == *framing
                && laid.size == size
                && laid.style == *style
                && same_cues(&laid.cues, &self.cues)
        });
        if same {
            return;
        }
        let layout = Layout {
            cues: self.cues.clone(),
            style: style.clone(),
            picture,
            framing: *framing,
            size,
        };
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
        let framing = Some(layout.framing);
        let text = overlay::text_overlay(
            &self.cues,
            &layout.style,
            &font,
            (width as f32, height as f32),
            framing,
            window,
        );
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
            // Its buttons drive the block as its own do, and a cast with it.
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

/// How long the pump waits before looking again: until the next frame is
/// due while one is, a little while one is being decoded, and long while
/// paused or with no picture that moves (a song's cover).
fn pump_wait(playing: bool, moving_picture: bool, next_frame: Option<Duration>) -> Duration {
    match (playing && moving_picture, next_frame) {
        (false, _) => PUMP_IDLE,
        (true, Some(due)) => due.clamp(PUMP_LEAST, PUMP_IDLE),
        (true, None) => PUMP_WAITING,
    }
}

/// Bring the block's window forward, and with it its Space when it is full
/// screen in one. Plays is often not the app in front then, as Picture in
/// Picture floats over every app and Space, and a window of an app behind
/// can't come forward alone: the app comes first.
fn bring_back(window: &mut Window, cx: &mut Context<PlayerView>) {
    cx.activate(true);
    window.activate_window();
    cx.emit(PlayerEvent::Activate);
}

/// Whether `laid` are the cues `showing`, each the very one.
fn same_cues(laid: &[Arc<CueContent>], showing: &[Arc<CueContent>]) -> bool {
    laid.len() == showing.len() && laid.iter().zip(showing).all(|(a, b)| Arc::ptr_eq(a, b))
}

#[cfg(test)]
#[path = "tests/pip.rs"]
mod tests;
