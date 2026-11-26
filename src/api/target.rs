//! What a command acts on: a player window, or a block in one. Each has an
//! id it keeps while it's open, a block from window to window as it moves,
//! that is never another's. A command names its window or block by
//! `Target`; one named by neither acts on the window in front and the
//! block in it that keys go to.
//!
//! A name is, in this order: an id, a label, a position (windows in the
//! order ⌘1 to ⌘9 count them, blocks in the order their grid shows them,
//! from 1), or the end of an id, as long as it's one id's.

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

use gpui::{AnyWindowHandle, App, Context, Entity, Window, WindowHandle};
use serde::{Deserialize, Serialize};

use super::{Error, Scope};
use crate::app::{last_front, player_windows};
use crate::ui::player::PlayerView;
use crate::ui::workspace::Workspace;
use crate::ui::workspace::all::Players;

/// A window's or a block's id: given once, and never to another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Id(u32);

/// The count of ids given, from which the next is made.
static GIVEN: AtomicU32 = AtomicU32::new(0);

impl Id {
    /// One not given before.
    pub fn next() -> Self {
        Self(spread(GIVEN.fetch_add(1, Ordering::Relaxed) + 1))
    }
}

/// `n` spread over every u32, one to one, so ids counted one after
/// another end differently and a few of their last digits name one.
/// (MurmurHash3's finalizer: each step can be undone.)
const fn spread(mut n: u32) -> u32 {
    n ^= n >> 16;
    n = n.wrapping_mul(0x85eb_ca6b);
    n ^= n >> 13;
    n = n.wrapping_mul(0xc2b2_ae35);
    n ^ (n >> 16)
}

/// Eight hex digits, such as `3fa9c2d1`.
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08x}", self.0)
    }
}

/// The window and the block a command is aimed at, by name; either may be
/// left out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
}

impl Target {
    pub fn is_empty(&self) -> bool {
        self.window.is_none() && self.block.is_none()
    }
}

/// One of the things a name may name: its id and label.
pub struct Named<'a> {
    pub id: Id,
    pub label: Option<&'a str>,
}

/// Which of `among` `name` names, a `what`: the one with that id, else
/// that label, else at that position (from 1) in `positions` (places in
/// `among`, in their order), else the one whose id ends so. `#` and a
/// number name by position alone, as ⌘1 to ⌘9 do: none is labeled so
/// or has an id ending so.
pub fn pick(what: &str, name: &str, among: &[Named], positions: &[usize]) -> Result<usize, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::not_found(what, "with no name"));
    }
    if let Some(number) = name.strip_prefix('#') {
        let place = number.parse::<usize>().ok().and_then(|n| n.checked_sub(1));
        return place
            .and_then(|place| positions.get(place).copied())
            .ok_or_else(|| Error::not_found(what, name));
    }
    if let Some(at) = among.iter().position(|named| named.id.to_string() == name) {
        return Ok(at);
    }
    if let Some(at) = one_of(what, name, among, |named| named.label == Some(name))? {
        return Ok(at);
    }
    let place = name.parse::<usize>().ok().and_then(|n| n.checked_sub(1));
    if let Some(&at) = place.and_then(|place| positions.get(place)) {
        return Ok(at);
    }
    let end = name.to_ascii_lowercase();
    one_of(what, name, among, |named| {
        named.id.to_string().ends_with(&end)
    })?
    .ok_or_else(|| Error::not_found(what, name))
}

/// The one of `among` that `matches`, if any; an error if several do.
fn one_of(
    what: &str,
    name: &str,
    among: &[Named],
    matches: impl Fn(&Named) -> bool,
) -> Result<Option<usize>, Error> {
    let found: Vec<usize> = (0..among.len()).filter(|&at| matches(&among[at])).collect();
    match found[..] {
        [] => Ok(None),
        [at] => Ok(Some(at)),
        _ => Err(Error::Ambiguous {
            what: what.to_owned(),
            name: name.to_owned(),
            matches: found.iter().map(|&at| among[at].id.to_string()).collect(),
        }),
    }
}

/// The player windows, in the order ⌘1 to ⌘9 count them.
pub fn windows(cx: &App) -> Vec<WindowHandle<Workspace>> {
    player_windows(cx)
        .into_iter()
        .filter_map(|window| window.downcast::<Workspace>())
        .collect()
}

/// The player window in front: the one active, else the frontmost of
/// them, else the one in front last, else any. One the system still says
/// is in front once it has closed isn't.
pub fn front_window(cx: &App) -> Option<WindowHandle<Workspace>> {
    let open = cx.windows();
    let player = |window: AnyWindowHandle| {
        open.contains(&window)
            .then(|| window.downcast::<Workspace>())
            .flatten()
    };
    cx.active_window()
        .and_then(player)
        .or_else(|| {
            cx.window_stack()
                .unwrap_or_default()
                .into_iter()
                .find_map(player)
        })
        .or_else(|| last_front(cx).and_then(player))
        .or_else(|| open.iter().copied().find_map(player))
}

/// The window `target` names, or the one in front.
pub fn window(target: &Target, cx: &App) -> Result<WindowHandle<Workspace>, Error> {
    let Some(name) = &target.window else {
        return front_window(cx).ok_or_else(|| Error::not_found("window", "in front"));
    };
    let windows: Vec<(WindowHandle<Workspace>, &Workspace)> = windows(cx)
        .into_iter()
        .filter_map(|window| Some((window, window.read(cx).ok()?)))
        .collect();
    let named: Vec<Named> = windows
        .iter()
        .map(|(_, workspace)| Named {
            id: workspace.id(),
            label: workspace.label(),
        })
        .collect();
    let positions: Vec<usize> = (0..named.len()).collect();
    let at = pick("window", name, &named, &positions)?;
    Ok(windows[at].0)
}

/// The block `target` names, and its window: in the window it names, or
/// with none named, in any by id, label or the end of its id, and in the
/// window in front by position. With no block named, the one keys go to
/// in the window named or in front.
pub fn block(
    target: &Target,
    cx: &App,
) -> Result<(WindowHandle<Workspace>, Entity<PlayerView>), Error> {
    let Some(name) = &target.block else {
        let window = window(target, cx)?;
        let active = read(window, cx)?.active_player();
        let id = || read(window, cx).map(Workspace::id);
        return match active {
            Some(block) => Ok((window, block)),
            None => Err(Error::not_found("block", format!("in window {}", id()?))),
        };
    };
    let (windows, front) = match target.window {
        Some(_) => (vec![window(target, cx)?], 0),
        None => {
            let windows = windows(cx);
            let front = front_window(cx).and_then(|front| windows.iter().position(|w| *w == front));
            (windows, front.unwrap_or(usize::MAX))
        }
    };
    let mut blocks = Vec::new();
    let mut positions = Vec::new();
    for (at, window) in windows.into_iter().enumerate() {
        for block in read(window, cx)?.players() {
            if at == front {
                positions.push(blocks.len());
            }
            blocks.push((window, block));
        }
    }
    let named: Vec<Named> = blocks
        .iter()
        .map(|(_, block)| {
            let view = block.read(cx);
            Named {
                id: view.id(),
                label: view.label(),
            }
        })
        .collect();
    let at = pick("block", name, &named, &positions)?;
    Ok(blocks.swap_remove(at))
}

/// `window`'s workspace, to read.
pub fn read(window: WindowHandle<Workspace>, cx: &App) -> Result<&Workspace, Error> {
    window.read(cx).map_err(|_| closed())
}

/// Run `f` on the window `target` names, or the one in front.
pub fn in_window<R>(
    target: &Target,
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) -> Result<R, Error>,
) -> Result<R, Error> {
    let window = window(target, cx)?;
    window.update(cx, f).map_err(|_| closed())?
}

/// The players a command of `scope` acts on: of every block in every
/// window, or in the window `target` names, or the one in front.
pub fn players(target: &Target, scope: Scope, cx: &App) -> Result<Players, Error> {
    if scope == Scope::EveryBlock {
        return Ok(Players::everywhere(cx));
    }
    Ok(Players::of(read(window(target, cx)?, cx)?))
}

/// Run `f` on the block `target` names, or the one keys go to, with its
/// window.
pub fn on_block<R>(
    target: &Target,
    cx: &mut App,
    f: impl FnOnce(&mut PlayerView, &mut Window, &mut Context<PlayerView>) -> Result<R, Error>,
) -> Result<R, Error> {
    let (window, block) = block(target, cx)?;
    window
        .update(cx, |_, window, cx| {
            block.update(cx, |player, cx| f(player, window, cx))
        })
        .map_err(|_| closed())?
}

/// What `read` makes of the block `target` names, or the one keys go to.
pub fn read_block<R>(
    target: &Target,
    cx: &App,
    read: impl FnOnce(&PlayerView, &App) -> R,
) -> Result<R, Error> {
    let (_, block) = block(target, cx)?;
    Ok(read(block.read(cx), cx))
}

fn closed() -> Error {
    Error::failed("the window has closed")
}

#[cfg(test)]
#[path = "tests/target.rs"]
mod tests;
