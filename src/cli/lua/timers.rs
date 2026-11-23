//! `plays.after` and `plays.every`: functions a script runs once some
//! seconds are up, or every so many, as it listens, never at once with
//! other Lua: one due as a handler runs waits for it, and a tick missed
//! meanwhile runs once. Each is a timer, stopped by `:cancel()`, running
//! on though the script drops it; its calls act on the script's window and
//! block, and a failure is said. A script has at most `MOST` going.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use mlua::{AnyUserData, Function, Lua, UserData, UserDataMethods};

/// How many timers a script may have going.
pub const MOST: usize = 1024;

/// A timer, as the script has it: `:cancel()` stops it.
struct Handle(Rc<Cell<bool>>);

impl UserData for Handle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("cancel", |_, handle, ()| {
            handle.0.set(true);
            Ok(())
        });
    }
}

/// A function to run once `due`, and then every `every`, if given.
struct Timer {
    function: Function,
    due: Instant,
    every: Option<Duration>,
    cancelled: Rc<Cell<bool>>,
}

/// The timers a script has going.
#[derive(Default)]
pub struct Timers {
    each: RefCell<Vec<Timer>>,
}

impl Timers {
    /// `plays.after` or, with `every`, `plays.every`: run `function` in
    /// `seconds`, and every `seconds` after with `every`.
    pub fn add(
        &self,
        lua: &Lua,
        (seconds, function): (f64, Function),
        every: bool,
    ) -> mlua::Result<AnyUserData> {
        let what = if every { "plays.every" } else { "plays.after" };
        let wait = Duration::try_from_secs_f64(seconds)
            .ok()
            .filter(|wait| !every || !wait.is_zero())
            .ok_or_else(|| mlua::Error::runtime(format!("{what} can't wait {seconds} seconds")))?;
        let cancelled = Rc::new(Cell::new(false));
        self.each.borrow_mut().push(Timer {
            function,
            due: Instant::now() + wait,
            every: every.then_some(wait),
            cancelled: cancelled.clone(),
        });
        lua.create_userdata(Handle(cancelled))
    }

    /// Forget those cancelled.
    fn forget_cancelled(&self) {
        self.each
            .borrow_mut()
            .retain(|timer| !timer.cancelled.get());
    }

    /// How many are going.
    pub fn count(&self) -> usize {
        self.forget_cancelled();
        self.each.borrow().len()
    }

    /// When the soonest is due, if any are going.
    pub fn next_due(&self) -> Option<Instant> {
        self.forget_cancelled();
        self.each.borrow().iter().map(|timer| timer.due).min()
    }

    /// The function of the soonest due by now, if any, rescheduled or
    /// forgotten: a timer late by more than its period runs once.
    pub fn take_due(&self, now: Instant) -> Option<Function> {
        self.forget_cancelled();
        let mut each = self.each.borrow_mut();
        let (at, _) = each
            .iter()
            .enumerate()
            .filter(|(_, timer)| timer.due <= now)
            .min_by_key(|(_, timer)| timer.due)?;
        let function = each[at].function.clone();
        match each[at].every {
            Some(every) => {
                let next = each[at].due + every;
                each[at].due = if next > now { next } else { now + every };
            }
            None => {
                each.remove(at);
            }
        }
        Some(function)
    }
}

#[cfg(test)]
#[path = "tests/timers.rs"]
mod tests;
