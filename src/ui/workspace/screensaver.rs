//! The empty window's screensaver. Left alone in front for a while
//! (Settings → Appearance), the window fades its buttons out and its logo
//! sets off across it, bouncing off its edges and taking the next color at
//! each, as old DVD players' did. Used again (the mouse moved, pressed or
//! scrolled in it, a key pressed, the window brought to the front or sent
//! behind), the logo flies back to its place and the buttons fade back in.
//! It keeps the color it had then. Clicked in its place, the logo takes
//! the next of those colors, and after the last its own again; it bounces
//! on from the one it has.
//!
//! Where the logo is follows from the time alone, read from gpui's
//! executor, so tests on its fake clock see it where it would be.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Hsla, Pixels, Point, Size, Task, WeakEntity, Window, canvas, point, px,
    rgb, size,
};

use super::Workspace;
use crate::settings::Settings;
use crate::ui::theme::screensaver::{COLOR_OPACITY, COLORS, FADE, RETURN, SPEED};

/// Whether the logo rests in its place, bounces about, or flies back.
#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Resting,
    Bouncing {
        since: Instant,
    },
    /// From where it was in the room.
    Returning {
        from: Point<Pixels>,
        since: Instant,
    },
}

pub(super) struct Screensaver {
    state: State,
    /// The window may rest, as last drawn.
    may: bool,
    /// When it was last used, or came to be able to rest.
    used_at: Instant,
    /// Waits for it to have rested long enough.
    watch: Option<Task<()>>,
    /// The empty window, and the logo's place in it, as last laid out.
    room: Bounds<Pixels>,
    home: Point<Pixels>,
    /// Where the logo was last drawn in the room.
    shown_at: Point<Pixels>,
    /// The logo's color in its place: one of `COLORS`, or none, for its
    /// own. A click on it takes the next; the screensaver ending, it keeps
    /// the one it had.
    resting_color: Option<usize>,
    /// The color the logo was last drawn in.
    shown_color: Option<usize>,
}

/// How the empty window draws at a moment.
#[derive(Debug, PartialEq)]
pub(super) struct Frame {
    /// Where the logo is in the room, while it's out of its place.
    pub logo_at: Option<Point<Pixels>>,
    /// The color it took at the edge it last bounced off.
    pub color: Option<Hsla>,
    /// How much the buttons show, from 0 to 1.
    pub buttons: f32,
}

impl Screensaver {
    pub fn new(now: Instant) -> Self {
        Self {
            state: State::Resting,
            may: false,
            used_at: now,
            watch: None,
            room: Bounds::default(),
            home: Point::default(),
            shown_at: Point::default(),
            resting_color: None,
            shown_color: None,
        }
    }

    pub fn bouncing(&self) -> bool {
        matches!(self.state, State::Bouncing { .. })
    }

    /// The color the logo was last drawn in.
    #[cfg(test)]
    pub fn shown_color(&self) -> Option<usize> {
        self.shown_color
    }

    /// The logo clicked: the next color, and after the last its own.
    fn next_color(&mut self) {
        self.resting_color = match self.resting_color {
            Some(last) if last + 1 == COLORS.len() => None,
            Some(picked) => Some(picked + 1),
            None => Some(0),
        };
    }

    /// The window was used: the logo flies back, if it was out.
    fn used(&mut self, now: Instant) {
        self.used_at = now;
        if let State::Bouncing { .. } = self.state {
            self.state = State::Returning {
                from: self.shown_at,
                since: now,
            };
            self.resting_color = self.shown_color;
        }
    }

    /// Whether the window `may` rest from `now`: it counts as resting only
    /// from when it may, and while it may not, nothing waits and the logo,
    /// if out, comes back.
    fn allow(&mut self, may: bool, now: Instant) {
        if may != self.may {
            self.may = may;
            self.used(now);
        }
        if !may {
            self.watch = None;
        }
    }

    /// The logo back in its place, once it has flown there: the window
    /// rests anew.
    fn settle(&mut self, now: Instant) {
        if let State::Returning { since, .. } = self.state
            && now.saturating_duration_since(since) >= RETURN
        {
            self.state = State::Resting;
            self.watch = None;
        }
    }

    /// How much longer the window has to rest, `after` it rested at all,
    /// before the logo sets off; none, as it sets off now.
    fn rest(&mut self, after: Duration, now: Instant) -> Option<Duration> {
        let rested = now.saturating_duration_since(self.used_at);
        if rested < after {
            return Some(after - rested);
        }
        self.state = State::Bouncing { since: now };
        self.shown_at = self.home - self.room.origin;
        None
    }

    /// The frame at `now`, the logo `logo` across.
    fn frame(&mut self, logo: Pixels, now: Instant) -> Frame {
        let home = self.home - self.room.origin;
        let (logo_at, nth, buttons) = match self.state {
            State::Resting => (None, self.resting_color, 1.),
            State::Bouncing { since } => {
                let gone = now.saturating_duration_since(since);
                let span = self.room.size - size(logo, logo);
                let (at, edges) = bounce(home, span, gone);
                // At each edge, the color after the one it had, round again
                // after the last.
                let from = self.resting_color.map_or(0, |nth| nth + 1);
                let nth = match edges {
                    0 => self.resting_color,
                    edges => Some((from + edges - 1) % COLORS.len()),
                };
                (Some(at), nth, 1. - part(gone, FADE))
            }
            State::Returning { from, since } => {
                let back = part(now.saturating_duration_since(since), RETURN);
                // Quick at first, slowing as it gets there.
                let eased = 1. - (1. - back).powi(3);
                (Some(from + (home - from) * eased), self.resting_color, back)
            }
        };
        if let Some(at) = logo_at {
            self.shown_at = at;
        }
        self.shown_color = nth;
        Frame {
            logo_at,
            color: nth.map(color),
            buttons,
        }
    }
}

/// Where the logo is, `gone` after it set off from `home`, across and down,
/// with `span` to move in each way; and how many edges it has bounced off.
fn bounce(home: Point<Pixels>, span: Size<Pixels>, gone: Duration) -> (Point<Pixels>, usize) {
    let travel = SPEED * gone.as_secs_f64();
    let (x, across) = fold(f64::from(home.x), travel, f64::from(span.width));
    let (y, down) = fold(f64::from(home.y), travel, f64::from(span.height));
    (point(px(x as f32), px(y as f32)), across + down)
}

/// Where something is that set off from `start` on a line `span` long,
/// gone `travel` along it, turning back at each end; and how many times it
/// turned.
fn fold(start: f64, travel: f64, span: f64) -> (f64, usize) {
    if span <= 0. {
        return (0., 0);
    }
    let at = start.clamp(0., span) + travel;
    let turns = (at / span).floor();
    let along = at - turns * span;
    let at = if turns % 2. == 0. {
        along
    } else {
        span - along
    };
    (at, turns as usize)
}

/// The `nth` of the logo's colors.
fn color(nth: usize) -> Hsla {
    Hsla::from(rgb(COLORS[nth])).opacity(COLOR_OPACITY)
}

/// How far `gone` is through `whole`, from 0 to 1.
fn part(gone: Duration, whole: Duration) -> f32 {
    (gone.as_secs_f32() / whole.as_secs_f32()).min(1.)
}

impl Workspace {
    /// The window was used: the mouse moved, pressed or scrolled in it, or
    /// a key was pressed. The logo, out, is drawn frame after frame, the
    /// next of them on its way back.
    pub(super) fn used(&mut self, cx: &App) {
        self.screensaver.used(cx.background_executor().now());
    }

    /// Whether the window may rest: it is empty and in front, no URL is
    /// being typed into it, and the screensaver is on. Each of those
    /// changing draws it.
    fn may_rest(&self, window: &Window, cx: &App) -> bool {
        self.tiles.is_empty()
            && !self.url_form_open
            && window.is_window_active()
            && cx.global::<Settings>().screensaver
    }

    /// As it's drawn: wait for the window to rest, while it may.
    pub(super) fn tend_screensaver(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let may = self.may_rest(window, cx);
        let saver = &mut self.screensaver;
        saver.allow(may, now);
        saver.settle(now);
        if may && saver.state == State::Resting && saver.watch.is_none() {
            saver.watch = Some(watch_rest(window, cx));
        }
    }

    /// The logo clicked in its place: its next color.
    pub(super) fn logo_clicked(&mut self, cx: &mut Context<Self>) {
        self.screensaver.next_color();
        cx.notify();
    }

    /// Another timeout: wait for it from the start.
    pub(super) fn screensaver_settings_changed(&mut self) {
        self.screensaver.watch = None;
    }

    /// The empty window's frame now, the logo `logo` across: more follow
    /// while the logo is out.
    pub(super) fn screensaver_frame(
        &mut self,
        logo: Pixels,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> Frame {
        let frame = self.screensaver.frame(logo, cx.background_executor().now());
        if frame.logo_at.is_some() {
            window.request_animation_frame();
        }
        frame
    }
}

/// Once the window has rested long enough, set the logo off; till then,
/// wait. Drawn while it may not rest, the window lets this go.
fn watch_rest(window: &mut Window, cx: &mut Context<Workspace>) -> Task<()> {
    cx.spawn_in(window, async move |this, cx| {
        loop {
            let wait = this.update_in(cx, |this, _, cx| {
                let after = cx.global::<Settings>().screensaver_after();
                let wait = this.screensaver.rest(after, cx.background_executor().now());
                if wait.is_none() {
                    cx.notify();
                }
                wait
            });
            match wait {
                Ok(Some(wait)) => cx.background_executor().timer(wait).await,
                _ => break,
            }
        }
    })
}

/// Notes where it's laid out: the room the logo bounces in, or its place.
pub(super) fn measure(
    workspace: WeakEntity<Workspace>,
    note: fn(&mut Screensaver, Bounds<Pixels>),
) -> impl IntoElement {
    canvas(
        move |bounds, _, cx| {
            workspace
                .update(cx, |this, _| note(&mut this.screensaver, bounds))
                .ok();
        },
        |_, (), _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Note the room the logo bounces in.
pub(super) fn note_room(saver: &mut Screensaver, bounds: Bounds<Pixels>) {
    saver.room = bounds;
}

/// Note the logo's place.
pub(super) fn note_home(saver: &mut Screensaver, bounds: Bounds<Pixels>) {
    saver.home = bounds.origin;
}

#[cfg(test)]
#[path = "tests/screensaver.rs"]
mod tests;
