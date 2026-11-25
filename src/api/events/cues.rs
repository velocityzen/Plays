//! Cues: a block reaching a time or a frame, told once, to the listener
//! that set it, as that frame becomes the block's: its name, such as
//! `block.3fa9c2d1.position@120.5s` or `block.intro.position@345f`, is
//! the event's type, and its payload is `block.playback.position`'s.
//!
//! A block reaches a cue as it plays from before it to it or past it,
//! frame by frame (`Watch`), or as it lands on it, within one frame: a
//! seek, a step, a scrub, the next loop or another file starting there.
//! One landing past it doesn't. A block past it when it's set reaches it
//! once it's back before it, looping or sought there, and plays on. Each
//! block watches its own as it ticks (`see`); a cue is gone once told,
//! once its listener stops, or once its block closes.

use std::cmp::Ordering;
use std::sync::Arc;

use futures::channel::mpsc::UnboundedSender;
use gpui::{App, Global};
use serde_json::{Value, json};

use super::seen::frame_at;
use super::{Event, playhead};
use crate::api::{Error, Id, Target, target};

/// What a cue's name has between the block and the time or frame.
const POSITION_AT: &str = ".position@";

/// How long a block that plays sound alone has to land on a time, past
/// it: a frame's time at 25 frames a second.
const SOUND_LANDING: f64 = 0.04;

/// Whether an event's type `kind` is a cue's: `block.<name>.position@…`.
pub fn is_cue(kind: &str) -> bool {
    kind.strip_prefix("block.")
        .is_some_and(|rest| rest.contains(POSITION_AT))
}

/// Where a cue is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum At {
    /// A time, in seconds.
    Time(f64),
    /// A frame, from 0, as `playhead` counts them.
    Frame(u64),
}

/// Where a block is, against a cue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Against {
    Before,
    /// At it: in the frame it is, or within a frame past the time it is.
    On,
    Past,
}

impl At {
    /// Where a block at `time` is against it, in a file of `frame_rate`:
    /// frames counted as `playhead` counts them. None for a frame, in one
    /// without frames.
    fn against(self, time: f64, frame_rate: Option<f64>) -> Option<Against> {
        let fps = frame_rate.filter(|&fps| fps > 0.0);
        let frame = fps.map_or(SOUND_LANDING, |fps| 1.0 / fps);
        let against = match self {
            Self::Frame(at) => match frame_at(time, fps?).cmp(&at) {
                Ordering::Less => Against::Before,
                Ordering::Equal => Against::On,
                Ordering::Greater => Against::Past,
            },
            Self::Time(at) if time < at => Against::Before,
            Self::Time(at) if time < at + frame => Against::On,
            Self::Time(_) => Against::Past,
        };
        Some(against)
    }
}

/// The block a cue named `name` is for, by name, and where it is; or why
/// that's no cue's name.
pub fn parse(name: &str) -> Result<(&str, At), String> {
    let wrong = |why: &str| format!("{name}: {why}");
    let rest = name
        .strip_prefix("block.")
        .ok_or_else(|| wrong("a cue is block.<block>.position@<time>s or …@<frame>f"))?;
    let (block, at) = rest
        .rsplit_once(POSITION_AT)
        .ok_or_else(|| wrong("a cue is block.<block>.position@<time>s or …@<frame>f"))?;
    if block.is_empty() {
        return Err(wrong("name the block, by its id or label"));
    }
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let at = if let Some(time) = at.strip_suffix('s') {
        let (whole, part) = time.split_once('.').unwrap_or((time, "0"));
        if !digits(whole) || !digits(part) {
            return Err(wrong("a time is seconds, such as 120 or 120.5, then s"));
        }
        At::Time(time.parse().map_err(|_| wrong("a time too far"))?)
    } else if let Some(frame) = at.strip_suffix('f') {
        if !digits(frame) {
            return Err(wrong("a frame is its number, from 0, then f"));
        }
        At::Frame(frame.parse().map_err(|_| wrong("a frame too far"))?)
    } else {
        return Err(wrong("end it with s, for seconds, or f, for a frame"));
    };
    Ok((block, at))
}

/// A block as a cue watches it, as it ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    /// Where it is, in seconds.
    pub time: f64,
    /// Counts what moves it other than playing on: files opened in it,
    /// seeks here or on the device it casts to, and its player's, loops
    /// and steps among them. One changing is a landing.
    pub moves: [u64; 3],
    pub duration: Option<f64>,
    /// None for sound alone.
    pub frame_rate: Option<f64>,
}

/// Whether a block has reached a cue, from what it was like when last
/// seen.
#[derive(Clone, Debug, PartialEq)]
pub struct Watch {
    at: At,
    last: Option<Last>,
}

/// A block as last seen.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Last {
    moves: [u64; 3],
    time: f64,
    /// It was before the cue.
    before: bool,
}

impl Watch {
    pub fn new(at: At) -> Self {
        Self { at, last: None }
    }

    /// Whether the block, `now`, has reached the cue since it was last
    /// seen: by playing on from before it to it or past it, or by landing
    /// on it, within a frame. Seen first, it hasn't; a frame isn't, in a
    /// file without frames.
    pub fn reached(&mut self, now: &Moment) -> bool {
        let Some(against) = self.at.against(now.time, now.frame_rate) else {
            return false;
        };
        let reached = self.last.is_some_and(|last| {
            if last.moves == now.moves {
                last.before && against != Against::Before
            } else {
                // Landing where it was isn't reaching it.
                now.time != last.time && against == Against::On
            }
        });
        self.last = Some(Last {
            moves: now.moves,
            time: now.time,
            before: against == Against::Before,
        });
        reached
    }
}

/// A cue set.
struct Cue {
    /// As it was set, the type it's told with.
    name: Arc<str>,
    block: Id,
    watch: Watch,
    /// The listener that set it.
    to: UnboundedSender<Event>,
}

/// The cues set, in the order they were.
#[derive(Default)]
struct Cues(Vec<Cue>);

impl Global for Cues {}

/// Tell `to` once the block the cue `name` names reaches it, in place of a
/// cue by that name it set before: from where the block is now. Hands back
/// the block's id and its window's; refuses a name that's no cue's, a
/// block not open, and a frame in a file opened without frames.
pub fn cue(name: &str, to: &UnboundedSender<Event>, cx: &mut App) -> Result<Value, Error> {
    let (block, at) = parse(name).map_err(|why| Error::bad_args("cue", why))?;
    let named = Target {
        window: None,
        block: Some(block.to_owned()),
    };
    let (window, view) = target::block(&named, cx)?;
    let view = view.read(cx);
    let (block, now) = (view.id(), view.moment());
    if matches!(at, At::Frame(_)) && now.is_some_and(|now| now.frame_rate.is_none()) {
        let why = format!("{name}: block {block} plays sound alone, with no frames");
        return Err(Error::bad_args("cue", why));
    }
    let window = target::read(window, cx)?.id();
    set(name, (block, at), now, to, cx);
    Ok(json!({"block": block.to_string(), "window": window.to_string()}))
}

/// Set the cue `name`, at `at` on `block`, told to `to` once reached, in
/// place of one by that name it set before: watched from `now`, as the
/// block is, if it's opened.
fn set(
    name: &str,
    (block, at): (Id, At),
    now: Option<Moment>,
    to: &UnboundedSender<Event>,
    cx: &mut App,
) {
    let mut watch = Watch::new(at);
    if let Some(now) = now {
        watch.reached(&now);
    }
    let cues = &mut cx.default_global::<Cues>().0;
    cues.retain(|cue| !cue.to.is_closed() && !(cue.to.same_receiver(to) && *cue.name == *name));
    cues.push(Cue {
        name: name.into(),
        block,
        watch,
        to: to.clone(),
    });
}

/// The cues `from` set are `to`'s from now on: a client subscribing
/// again keeps its cues.
pub fn hand_over(from: &UnboundedSender<Event>, to: &UnboundedSender<Event>, cx: &mut App) {
    if let Some(cues) = cx.try_global::<Cues>()
        && cues.0.iter().any(|cue| cue.to.same_receiver(from))
    {
        for cue in &mut cx.global_mut::<Cues>().0 {
            if cue.to.same_receiver(from) {
                cue.to = to.clone();
            }
        }
    }
}

/// Whether any cue is set on `block`.
pub fn watched(block: Id, cx: &App) -> bool {
    cx.try_global::<Cues>()
        .is_some_and(|cues| cues.0.iter().any(|cue| cue.block == block))
}

/// `block` ticked, and is as `now` says: the cues on it it has reached
/// are told, once what's ticking has finished, and gone.
pub fn see(block: Id, now: Moment, cx: &mut App) {
    if !cx.has_global::<Cues>() {
        return;
    }
    let mut reached = Vec::new();
    cx.global_mut::<Cues>().0.retain_mut(|cue| {
        if cue.to.is_closed() {
            return false;
        }
        if cue.block != block || !cue.watch.reached(&now) {
            return true;
        }
        reached.push((Arc::clone(&cue.name), cue.to.clone()));
        false
    });
    if !reached.is_empty() {
        cx.defer(move |cx| tell(block, now, reached, cx));
    }
}

/// Forget the cues on blocks not among `open`, closed.
pub fn forget_closed(open: &[Id], cx: &mut App) {
    if cx.has_global::<Cues>() {
        cx.global_mut::<Cues>()
            .0
            .retain(|cue| !cue.to.is_closed() && open.contains(&cue.block));
    }
}

/// Tell each listener in `reached` its cue, `block` being where `now`
/// says, with its window's id and title and its own, as now.
fn tell(block: Id, now: Moment, reached: Vec<(Arc<str>, UnboundedSender<Event>)>, cx: &App) {
    let named = Target {
        window: None,
        block: Some(block.to_string()),
    };
    let Ok((window, view)) = target::block(&named, cx) else {
        return;
    };
    let Ok(workspace) = target::read(window, cx) else {
        return;
    };
    let at = playhead(now.time, now.duration, now.frame_rate);
    let payload = json!({
        "window_id": workspace.id().to_string(),
        "window_title": workspace.title(),
        "block_id": block.to_string(),
        "block_title": view.read(cx).header_title().to_string(),
        "position": at["position"],
        "time": at["time"],
        "frame": at["frame"],
        "duration": now.duration,
    });
    for (name, to) in reached {
        to.unbounded_send(Event::cue(name, payload.clone())).ok();
    }
}

#[cfg(test)]
#[path = "tests/cues.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/cues_properties.rs"]
mod properties;
