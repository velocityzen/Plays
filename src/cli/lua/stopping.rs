//! `plays.on_stop`: a function a script runs once as it ends, however it
//! ends but as Plays goes; and Ctrl-C, caught while a script has one, so
//! it stops where it is and the function runs. A second Ctrl-C, or one
//! with no function to run, ends it at once, as it would uncaught.
//!
//! Ctrl-C is the process's: the signal handler writes a byte to a pipe,
//! and a thread of its own reads it and wakes what waits.

use std::cell::RefCell;
use std::ffi::CStr;
use std::io::{self, PipeReader, Read};
use std::os::fd::IntoRawFd;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use libc::c_int;
use mlua::{Function, Lua, ffi};

use super::inbox::Came;

/// What a script stopped by Ctrl-C fails with, step after step.
pub const INTERRUPTED: &str = match RAISED.to_str() {
    Ok(text) => text,
    Err(_) => panic!("INTERRUPTED is UTF-8"),
};

/// `INTERRUPTED`, as Lua's hook raises it.
const RAISED: &CStr = c"interrupted";

/// The exit status of a script Ctrl-C stopped, as the shell gives one a
/// signal ends.
pub const INTERRUPTED_STATUS: u8 = 130;

/// How many instructions Lua runs between looks at whether Ctrl-C came.
const LOOK_EVERY: c_int = 1000;

/// Whether Ctrl-C was caught: what runs stops where it is.
static CAUGHT: AtomicBool = AtomicBool::new(false);

/// How many scripts have a function to run as they stop: with none,
/// Ctrl-C ends the process at once.
static WANTED: AtomicUsize = AtomicUsize::new(0);

/// The pipe's end the signal handler writes to.
static WAKE: AtomicI32 = AtomicI32::new(-1);

/// Where what waits for events, or for programs, is woken: each script's
/// inbox, by the script's number.
static WAITING: Mutex<Vec<(u64, Sender<Came>)>> = Mutex::new(Vec::new());

/// The number the next script catching Ctrl-C has.
static NEXT: AtomicU64 = AtomicU64::new(0);

/// Where sleeps are woken: the lock `CAUGHT` is set under.
static SLEEPS: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());

/// The function a script runs as it stops, if it gave one, and whether
/// Ctrl-C is caught for it.
#[derive(Default)]
pub struct OnStop {
    function: RefCell<Option<Function>>,
    /// The script's number, as it catches Ctrl-C.
    catching: RefCell<Option<u64>>,
}

impl OnStop {
    /// `plays.on_stop(fn)`: run `fn` as the script ends, in place of any
    /// given before; nil runs none. Ctrl-C is caught from then on.
    pub fn set(
        &self,
        lua: &Lua,
        function: Option<Function>,
        inbox: Sender<Came>,
    ) -> mlua::Result<()> {
        if function.is_some() && self.catching.borrow().is_none() {
            let number = catch(lua, inbox)?;
            self.catching.replace(Some(number));
        } else if function.is_none() {
            self.let_go();
        }
        self.function.replace(function);
        Ok(())
    }

    /// The function to run as the script stops, if any, taken: it runs
    /// once. Ctrl-C isn't caught as it runs.
    pub fn take(&self) -> Option<Function> {
        self.let_go();
        self.function.take()
    }

    /// Ctrl-C ends the process at once again, but for other scripts'.
    fn let_go(&self) {
        if let Some(number) = self.catching.take() {
            WANTED.fetch_sub(1, Ordering::SeqCst);
            let mut waiting = WAITING.lock().unwrap_or_else(PoisonError::into_inner);
            waiting.retain(|(of, _)| *of != number);
        }
    }
}

impl Drop for OnStop {
    fn drop(&mut self) {
        self.let_go();
    }
}

/// Whether Ctrl-C was caught, and what runs is to stop.
pub fn caught() -> bool {
    CAUGHT.load(Ordering::SeqCst)
}

/// Fine, unless Ctrl-C was caught: then failing, so what runs stops
/// there, and nothing more is asked of Plays.
pub fn unless_caught() -> mlua::Result<()> {
    if caught() {
        Err(mlua::Error::runtime(INTERRUPTED))
    } else {
        Ok(())
    }
}

/// As the function run on stopping begins: it waits and sleeps as it says,
/// though Ctrl-C was caught.
pub fn calm(lua: &Lua) {
    CAUGHT.store(false, Ordering::SeqCst);
    lua.remove_hook();
}

/// Sleep `duration`, or until Ctrl-C is caught.
pub fn sleep(duration: Duration) {
    let (lock, woken) = &SLEEPS;
    let held = lock.lock().unwrap_or_else(PoisonError::into_inner);
    drop(woken.wait_timeout_while(held, duration, |()| !caught()));
}

/// Catch Ctrl-C for a script whose waits `inbox` wakes, and stop its Lua
/// where it is once it's caught: the script's number.
fn catch(lua: &Lua, inbox: Sender<Came>) -> mlua::Result<u64> {
    static HEARING: OnceLock<io::Result<()>> = OnceLock::new();
    if let Err(err) = HEARING.get_or_init(hear) {
        let why = format!("plays.on_stop can't catch Ctrl-C: {err}");
        return Err(mlua::Error::runtime(why));
    }
    // SAFETY: exec_raw's function only sets the hook, which raises
    // nothing, on the state it's given, and keeps nothing of it.
    unsafe {
        lua.exec_raw::<()>((), |state| {
            ffi::lua_sethook(state, Some(interrupting), ffi::LUA_MASKCOUNT, LOOK_EVERY);
        })?;
    }
    let number = NEXT.fetch_add(1, Ordering::SeqCst);
    WAITING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((number, inbox));
    WANTED.fetch_add(1, Ordering::SeqCst);
    handle(caught_signal as *const () as libc::sighandler_t);
    Ok(number)
}

/// Lua's hook, every `LOOK_EVERY` instructions, as a script catches
/// Ctrl-C: once it's caught, `INTERRUPTED` raised where Lua is, as Lua's
/// own interpreter stops at Ctrl-C. A hook of Lua's, not mlua's: mlua's
/// raises with its error in the running function's first register, where
/// a closure over that local, its `plays.on_stop` function's, sees it.
unsafe extern "C-unwind" fn interrupting(state: *mut ffi::lua_State, _: *mut ffi::lua_Debug) {
    if caught() {
        // SAFETY: as a hook runs, Lua keeps the stack's top above the
        // running function's registers, so the message is pushed past
        // them; and a hook may raise, as a C function does.
        unsafe {
            ffi::lua_pushstring(state, RAISED.as_ptr());
            ffi::lua_error(state);
        }
    }
}

/// The pipe the signal handler writes to, and the thread that reads it,
/// started.
fn hear() -> io::Result<()> {
    let (reader, writer) = io::pipe()?;
    // Kept open for the process: the handler may write to it any time.
    WAKE.store(writer.into_raw_fd(), Ordering::SeqCst);
    std::thread::Builder::new()
        .name("ctrl-c".into())
        .spawn(move || heard(reader))?;
    Ok(())
}

/// Each Ctrl-C, as the pipe says it: what waits woken, the script to stop;
/// or, with no script to stop, the process ended, as Ctrl-C would.
fn heard(mut reader: PipeReader) {
    let mut byte = [0];
    while reader.read_exact(&mut byte).is_ok() {
        // The next ends the process at once.
        handle(libc::SIG_DFL);
        if WANTED.load(Ordering::SeqCst) == 0 {
            // SAFETY: raise(3) takes any signal number; SIGINT, handled as
            // by default now, ends the process as Ctrl-C does.
            unsafe { libc::raise(libc::SIGINT) };
            continue;
        }
        {
            let (lock, woken) = &SLEEPS;
            let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
            CAUGHT.store(true, Ordering::SeqCst);
            woken.notify_all();
        }
        let waiting = WAITING.lock().unwrap_or_else(PoisonError::into_inner);
        for (_, inbox) in waiting.iter() {
            inbox.send(Came::Interrupted).ok();
        }
    }
}

/// Handle SIGINT with `handler`: `caught_signal`, or the default.
fn handle(handler: libc::sighandler_t) {
    // SAFETY: signal(2) with SIGINT and either the default action or
    // `caught_signal`, which only writes to a pipe: async-signal-safe.
    unsafe { libc::signal(libc::SIGINT, handler) };
}

/// SIGINT's handler: a byte to the pipe, for the thread that hears it.
extern "C" fn caught_signal(_: libc::c_int) {
    let fd = WAKE.load(Ordering::SeqCst);
    if fd >= 0 {
        let byte = 1u8;
        // SAFETY: write(2) is async-signal-safe, and `byte` is a live byte
        // on this stack; the pipe is open for the process.
        unsafe { libc::write(fd, (&raw const byte).cast(), 1) };
    }
}

#[cfg(test)]
#[path = "tests/stopping.rs"]
mod tests;
