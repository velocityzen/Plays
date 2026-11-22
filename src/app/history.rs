//! When each of some things, windows or the tiles in one, was last in
//! front, to go back to them: the newest but the one in front
//! (`recent`), or walking back from it (`back`). As on a desktop, going
//! back to one puts it in front, newest of all: this isn't undo.

/// When each was last in front, by a count that only goes up, and a walk
/// back through them under way.
#[derive(Debug)]
pub struct FocusHistory<T> {
    count: u64,
    fronts: Vec<(T, u64)>,
    walk: Option<Walk<T>>,
}

/// A walk back: what it last went to, and when that was in front before.
/// Only those in front before then are further back, so a walk goes to
/// each once.
#[derive(Clone, Copy, Debug)]
struct Walk<T> {
    at: T,
    before: u64,
}

impl<T> Default for FocusHistory<T> {
    fn default() -> Self {
        Self {
            count: 0,
            fronts: Vec::new(),
            walk: None,
        }
    }
}

impl<T: Copy + PartialEq> FocusHistory<T> {
    /// `front` came to the front: a walk back that didn't bring it there
    /// is over. Those `gone` are forgotten.
    pub fn note(&mut self, front: T, gone: impl Fn(T) -> bool) {
        if self.walk.as_ref().is_some_and(|walk| walk.at != front) {
            self.walk = None;
        }
        self.count += 1;
        self.fronts
            .retain(|&(other, _)| other != front && !gone(other));
        self.fronts.push((front, self.count));
    }

    /// The newest in front but `front`, of those `open`.
    pub fn recent(&self, front: Option<T>, open: impl Fn(T) -> bool) -> Option<T> {
        self.last_before(u64::MAX, front, &open)
    }

    /// The one in front before `front`, of those `open`; walking on, while
    /// `front` is where the walk last went, the one before that, and so
    /// on. Noted as the walk's next step; bring it to the front, and note
    /// it there.
    pub fn back(&mut self, front: Option<T>, walking: bool, open: impl Fn(T) -> bool) -> Option<T> {
        let before = self
            .walk
            .as_ref()
            .filter(|walk| walking && Some(walk.at) == front)
            .map_or(u64::MAX, |walk| walk.before);
        let (at, when) = self.last_before(before, front, &open).map(|at| {
            let when = self.when(at).unwrap_or(0);
            (at, when)
        })?;
        self.walk = Some(Walk { at, before: when });
        Some(at)
    }

    fn when(&self, of: T) -> Option<u64> {
        self.fronts
            .iter()
            .find(|&&(other, _)| other == of)
            .map(|&(_, when)| when)
    }

    /// The newest in front before `before`, but `front`, of those `open`.
    fn last_before(&self, before: u64, front: Option<T>, open: &impl Fn(T) -> bool) -> Option<T> {
        self.fronts
            .iter()
            .filter(|&&(one, when)| when < before && Some(one) != front && open(one))
            .max_by_key(|&&(_, when)| when)
            .map(|&(one, _)| one)
    }
}

#[cfg(test)]
#[path = "tests/history.rs"]
mod tests;
