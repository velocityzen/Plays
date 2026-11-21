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
mod tests {
    use super::*;

    #[test]
    fn a_line_is_gone_back_and_forth() {
        // Within it, from where it started.
        assert_eq!(fold(2., 3., 10.), (5., 0));
        // To its far end, and back from it.
        assert_eq!(fold(2., 8., 10.), (10., 1));
        assert_eq!(fold(2., 12., 10.), (6., 1));
        // To its near end, and on again.
        assert_eq!(fold(2., 18., 10.), (0., 2));
        assert_eq!(fold(2., 21., 10.), (3., 2));
        assert_eq!(fold(0., 35., 10.), (5., 3));
        // Started past an end, it starts there.
        assert_eq!(fold(15., 1., 10.), (9., 1));
        // No room: it stays put.
        assert_eq!(fold(3., 7., 0.), (0., 0));
        assert_eq!(fold(3., 7., -4.), (0., 0));
    }

    /// A saver over a room 400 × 300 at (100, 50), the logo's place at
    /// (150, 100) in it.
    fn saver(now: Instant) -> Screensaver {
        let mut saver = Screensaver::new(now);
        saver.room = Bounds::new(point(px(100.), px(50.)), size(px(400.), px(300.)));
        saver.home = point(px(250.), px(150.));
        saver
    }

    const LOGO: Pixels = px(100.);

    fn seconds(s: f64) -> Duration {
        Duration::from_secs_f64(s)
    }

    #[test]
    fn the_logo_sets_off_once_rested_and_bounces_taking_colors() {
        let start = Instant::now();
        let mut saver = saver(start);
        let after = seconds(60.);
        assert_eq!(saver.rest(after, start + seconds(20.)), Some(seconds(40.)));
        assert_eq!(saver.frame(LOGO, start + seconds(20.)).logo_at, None);
        assert_eq!(saver.rest(after, start + seconds(60.)), None);
        assert!(saver.bouncing());

        let set_off = start + seconds(60.);
        let at = |saver: &mut Screensaver, s: f64| saver.frame(LOGO, set_off + seconds(s));
        let first = at(&mut saver, 0.);
        assert_eq!(first.logo_at, Some(point(px(150.), px(100.))));
        assert_eq!((first.color, first.buttons), (None, 1.), "as it was");

        let fading = at(&mut saver, FADE.as_secs_f64() / 2.);
        assert!((fading.buttons - 0.5).abs() < 1e-3, "{}", fading.buttons);
        let faded = at(&mut saver, FADE.as_secs_f64() * 2.);
        assert!(faded.buttons.abs() < f32::EPSILON);

        // 150 across and 100 down to go: the bottom first, then the right.
        let to_bottom = 100. / SPEED;
        let before = at(&mut saver, to_bottom - 0.1);
        assert_eq!(before.color, None);
        let bottom = at(&mut saver, to_bottom + 0.1);
        assert_eq!(bottom.color, Some(color(0)));
        let right = at(&mut saver, 150. / SPEED + 0.1);
        assert_eq!(right.color, Some(color(1)));
        assert_ne!(color(0), color(1));
        let right_at = right.logo_at.unwrap();
        assert!(
            (f64::from(right_at.x) - (300. - 0.1 * SPEED)).abs() < 0.01,
            "{right_at:?}"
        );
        assert!(
            (f64::from(right_at.y) - (200. - 50. - 0.1 * SPEED)).abs() < 0.01,
            "{right_at:?}"
        );

        // As long as it goes, it stays in the room, and takes each color
        // in turn (two at once in a corner), round again after the last.
        let mut colors = Vec::new();
        for tenth in 0..10_000 {
            let at = at(&mut saver, f64::from(tenth) / 10.).logo_at.unwrap();
            assert!(at.x >= px(0.) && at.x <= px(300.), "{at:?}");
            assert!(at.y >= px(0.) && at.y <= px(200.), "{at:?}");
            if colors.last() != Some(&saver.shown_color) {
                colors.push(saver.shown_color);
            }
        }
        let colors: Vec<usize> = colors.into_iter().flatten().collect();
        assert_eq!(colors[0], 0, "{colors:?}");
        for two in colors.windows(2) {
            let next = (two[0] + 1) % COLORS.len();
            assert!(
                two[1] == next || two[1] == (next + 1) % COLORS.len(),
                "{colors:?}"
            );
        }
        assert!(colors.windows(2).any(|two| two[1] < two[0]), "{colors:?}");
    }

    #[test]
    fn used_the_logo_flies_back_quickly_and_the_buttons_fade_in() {
        let start = Instant::now();
        let mut saver = saver(start);
        assert_eq!(saver.rest(seconds(0.), start), None);
        let out = saver.frame(LOGO, start + seconds(1.)).logo_at.unwrap();
        assert_ne!(out, point(px(150.), px(100.)));

        let woke = start + seconds(2.);
        saver.used(woke);
        assert!(!saver.bouncing());
        // Used again on its way, it goes on.
        saver.used(woke);
        let back = |saver: &mut Screensaver, s: f64| saver.frame(LOGO, woke + seconds(s));
        // From where it was last drawn, in the color it was drawn in: its
        // own, before the first edge, though it would be past it by now.
        let first = back(&mut saver, 0.);
        assert_eq!((first.logo_at, first.buttons), (Some(out), 0.));
        assert_eq!(first.color, None);
        let half = back(&mut saver, RETURN.as_secs_f64() / 2.);
        assert!((half.buttons - 0.5).abs() < 1e-3);
        // Most of the way there in half the time.
        let home = point(px(150.), px(100.));
        let gone = |at: Point<Pixels>| f32::from((at - out).x).hypot(f32::from((at - out).y));
        let whole = gone(home);
        assert!(gone(half.logo_at.unwrap()) > whole * 0.8);
        let there = back(&mut saver, RETURN.as_secs_f64());
        assert_eq!((there.logo_at, there.buttons), (Some(home), 1.));

        saver.settle(woke + RETURN);
        assert_eq!(saver.frame(LOGO, woke + RETURN).logo_at, None);
        assert_eq!(
            saver.rest(seconds(60.), woke + RETURN),
            Some(seconds(60.) - RETURN),
            "resting from when it was used"
        );
    }

    #[test]
    fn a_click_picks_the_next_color_and_it_bounces_on_from_it() {
        let start = Instant::now();
        let mut saver = saver(start);
        assert_eq!(saver.frame(LOGO, start).color, None, "its own");
        let mut picked = Vec::new();
        for _ in 0..=COLORS.len() {
            saver.next_color();
            picked.push(saver.frame(LOGO, start).color);
        }
        let each: Vec<_> = (0..COLORS.len())
            .map(|nth| Some(color(nth)))
            .chain([None])
            .collect();
        assert_eq!(picked, each, "each in turn, then its own again");

        // With the third picked, it sets off in it, and takes the fourth at
        // the bottom edge and the fifth at the right.
        for _ in 0..3 {
            saver.next_color();
        }
        assert_eq!(saver.rest(seconds(0.), start), None);
        let at = |saver: &mut Screensaver, s: f64| saver.frame(LOGO, start + seconds(s)).color;
        assert_eq!(at(&mut saver, 0.), Some(color(2)));
        assert_eq!(at(&mut saver, 100. / SPEED + 0.1), Some(color(3)));
        assert_eq!(at(&mut saver, 150. / SPEED + 0.1), Some(color(4)));
        // Brought back, it keeps the one it was last drawn in, the fifth,
        // and sets off again in it, taking the sixth, then the first.
        saver.used(start + seconds(10.));
        assert_eq!(at(&mut saver, 10.), Some(color(4)));
        saver.settle(start + seconds(10.) + RETURN);
        assert_eq!(at(&mut saver, 11.), Some(color(4)));
        assert_eq!(saver.rest(seconds(0.), start + seconds(11.)), None);
        let again = |saver: &mut Screensaver, s: f64| at(saver, 11. + s);
        assert_eq!(again(&mut saver, 0.), Some(color(4)));
        assert_eq!(again(&mut saver, 100. / SPEED + 0.1), Some(color(5)));
        assert_eq!(again(&mut saver, 150. / SPEED + 0.1), Some(color(0)));
    }

    #[test]
    fn it_rests_only_from_when_it_may() {
        let start = Instant::now();
        let mut saver = saver(start);
        saver.allow(true, start);
        saver.allow(false, start + seconds(10.));
        // Long after, still not allowed: then allowed.
        saver.allow(false, start + seconds(100.));
        saver.allow(true, start + seconds(200.));
        assert_eq!(
            saver.rest(seconds(60.), start + seconds(230.)),
            Some(seconds(30.))
        );

        // Not allowed while out, before it was drawn out: it comes back
        // from its place.
        assert_eq!(saver.rest(seconds(60.), start + seconds(260.)), None);
        saver.allow(false, start + seconds(261.));
        assert!(!saver.bouncing());
        let back = saver.frame(LOGO, start + seconds(261.));
        assert_eq!(back.logo_at, Some(point(px(150.), px(100.))));
    }
}
