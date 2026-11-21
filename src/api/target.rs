//! What a command acts on: a player window, or a tile in one. Each has an
//! id it keeps while it's open, a tile from window to window as it moves,
//! that is never another's. A command names its window or tile by
//! `Target`; one named by neither acts on the window in front and the
//! tile in it that keys go to.
//!
//! A name is, in this order: an id, a label, a position (windows in the
//! order ⌘1 to ⌘9 count them, tiles in the order their grid shows them,
//! from 1), or the end of an id, as long as it's one id's.

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

use gpui::{AnyWindowHandle, App, Context, Entity, Window, WindowHandle};
use serde::{Deserialize, Serialize};

use super::Error;
use crate::app::player_windows;
use crate::ui::player::PlayerView;
use crate::ui::workspace::Workspace;

/// A window's or a tile's id: given once, and never to another.
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

/// The window and the tile a command is aimed at, by name; either may be
/// left out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile: Option<String>,
}

impl Target {
    pub fn is_empty(&self) -> bool {
        self.window.is_none() && self.tile.is_none()
    }
}

/// One of the things a name may name: its id and label.
pub struct Named<'a> {
    pub id: Id,
    pub label: Option<&'a str>,
}

/// Which of `among` `name` names, a `what`: the one with that id, else
/// that label, else at that position (from 1) in `positions` (places in
/// `among`, in their order), else the one whose id ends so.
pub fn pick(what: &str, name: &str, among: &[Named], positions: &[usize]) -> Result<usize, Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::not_found(what, "with no name"));
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
/// them, else any. One the system still says is in front once it has
/// closed isn't.
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

/// The tile `target` names, and its window: in the window it names, or
/// with none named, in any by id, label or the end of its id, and in the
/// window in front by position. With no tile named, the one keys go to
/// in the window named or in front.
pub fn tile(
    target: &Target,
    cx: &App,
) -> Result<(WindowHandle<Workspace>, Entity<PlayerView>), Error> {
    let Some(name) = &target.tile else {
        let window = window(target, cx)?;
        let active = read(window, cx)?.active_player();
        let id = || read(window, cx).map(Workspace::id);
        return match active {
            Some(tile) => Ok((window, tile)),
            None => Err(Error::not_found("tile", format!("in window {}", id()?))),
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
    let mut tiles = Vec::new();
    let mut positions = Vec::new();
    for (at, window) in windows.into_iter().enumerate() {
        for tile in read(window, cx)?.players() {
            if at == front {
                positions.push(tiles.len());
            }
            tiles.push((window, tile));
        }
    }
    let named: Vec<Named> = tiles
        .iter()
        .map(|(_, tile)| {
            let view = tile.read(cx);
            Named {
                id: view.id(),
                label: view.label(),
            }
        })
        .collect();
    let at = pick("tile", name, &named, &positions)?;
    Ok(tiles.swap_remove(at))
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

/// Run `f` on the tile `target` names, or the one keys go to, with its
/// window.
pub fn on_tile<R>(
    target: &Target,
    cx: &mut App,
    f: impl FnOnce(&mut PlayerView, &mut Window, &mut Context<PlayerView>) -> Result<R, Error>,
) -> Result<R, Error> {
    let (window, tile) = tile(target, cx)?;
    window
        .update(cx, |_, window, cx| {
            tile.update(cx, |player, cx| f(player, window, cx))
        })
        .map_err(|_| closed())?
}

/// What `read` makes of the tile `target` names, or the one keys go to.
pub fn read_tile<R>(
    target: &Target,
    cx: &App,
    read: impl FnOnce(&PlayerView, &App) -> R,
) -> Result<R, Error> {
    let (_, tile) = tile(target, cx)?;
    Ok(read(tile.read(cx), cx))
}

fn closed() -> Error {
    Error::failed("the window has closed")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn named(ids: &[u32], labels: &[Option<&'static str>]) -> Vec<Named<'static>> {
        ids.iter()
            .zip(labels)
            .map(|(&id, &label)| Named { id: Id(id), label })
            .collect()
    }

    #[test]
    fn ids_are_never_given_twice_and_end_differently() {
        let ids: Vec<Id> = (0..10_000).map(|_| Id::next()).collect();
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
        // Counted one after another, yet a few last digits tell them apart.
        let ends: HashSet<String> = (1..=16)
            .map(|n| Id(spread(n)).to_string()[5..].to_owned())
            .collect();
        assert_eq!(ends.len(), 16);
        assert_eq!(Id(0x3fa9_c2d1).to_string(), "3fa9c2d1");
        assert_eq!(Id(7).to_string(), "00000007");
    }

    #[test]
    fn names_by_id_then_label_then_position_then_the_end_of_the_id() {
        let among = named(
            &[0x1111_aaaa, 0x2222_bbbb, 0x3333_cccc, 0x4444_dddd],
            &[Some("left"), None, Some("2"), Some("2222bbbb")],
        );
        let positions = [0, 1, 2, 3];
        let pick = |name| pick("tile", name, &among, &positions);
        assert_eq!(pick("2222bbbb"), Ok(1), "by id, before a label");
        assert_eq!(pick("left"), Ok(0), "by label");
        assert_eq!(pick("2"), Ok(2), "a label before a position");
        assert_eq!(pick("1"), Ok(0), "by position, from 1");
        assert_eq!(pick("3"), Ok(2));
        assert_eq!(pick("cccc"), Ok(2), "by the end of its id");
        assert_eq!(pick("BBB"), Ok(1), "in either case");
        assert_eq!(pick(" left "), Ok(0), "spaces aside");
        assert_eq!(pick("5"), Err(Error::not_found("tile", "5")));
        assert_eq!(pick("eeee"), Err(Error::not_found("tile", "eeee")));
        assert_eq!(pick(""), Err(Error::not_found("tile", "with no name")));
    }

    #[test]
    fn a_name_that_could_be_several_lists_them() {
        let among = named(
            &[0x1111_00a1, 0x2222_00a1, 0x3333_00b1],
            &[Some("news"), Some("news"), None],
        );
        let positions = [0, 1, 2];
        assert_eq!(
            pick("tile", "a1", &among, &positions),
            Err(Error::Ambiguous {
                what: "tile".into(),
                name: "a1".into(),
                matches: vec!["111100a1".into(), "222200a1".into()],
            })
        );
        assert_eq!(
            pick("window", "news", &among, &positions),
            Err(Error::Ambiguous {
                what: "window".into(),
                name: "news".into(),
                matches: vec!["111100a1".into(), "222200a1".into()],
            })
        );
        assert_eq!(pick("tile", "b1", &among, &positions), Ok(2));
    }

    #[test]
    fn positions_count_only_those_given() {
        // Every tile, of which the window in front has the last two.
        let among = named(&[0x10, 0x20, 0x31], &[None, None, None]);
        assert_eq!(pick("tile", "1", &among, &[1, 2]), Ok(1));
        assert_eq!(pick("tile", "2", &among, &[1, 2]), Ok(2));
        assert_eq!(pick("tile", "00000010", &among, &[1, 2]), Ok(0), "by id");
        // Not a 31st there: the end of an id instead.
        assert_eq!(pick("tile", "31", &among, &[1, 2]), Ok(2));
        assert_eq!(
            pick("tile", "0", &among, &[1, 2]),
            Err(Error::Ambiguous {
                what: "tile".into(),
                name: "0".into(),
                matches: vec!["00000010".into(), "00000020".into()],
            }),
            "not a position, and two ids end so"
        );
    }
}
