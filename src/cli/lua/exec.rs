//! `plays.exec`: programs a script runs, with no shell. One it waits for,
//! what it wrote and how it ended coming back; or, with `plays.exec.start`,
//! one it starts, waited for by its handle's `:wait()`, its output going,
//! as it comes, to functions, if given, run as the script waits for a
//! program or listens, never at once with other Lua.
//!
//! Each stream is read on a thread of its own, and a thread watches each
//! program, killing it once its timeout is up, or as the part of the script
//! that started it ends, its top level, a handler or a timer, unless it's
//! detached; as the script ends, every one. What they do comes to the
//! script's inbox (`inbox`).

use std::cell::{Cell, RefCell};
use std::fmt;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mlua::{Function, Lua, Scope, Table, UserData, UserDataMethods, Value as LuaValue};

use super::Answer;
use super::inbox::{Came, Inbox};
use crate::cli::after;

/// How often a program's watcher looks whether it's ended.
const LOOK_EVERY: Duration = Duration::from_millis(10);

/// How long a program killed for its timeout may take to close its
/// streams: those a program it started keeps open are let go then.
const LET_GO: Duration = Duration::from_millis(500);

/// How much of a stream is read at a time, at most.
const PIECE: usize = 64 * 1024;

/// The part of a script that's its top level.
const TOP_LEVEL: u64 = 0;

/// What a program did.
#[derive(Debug)]
pub enum Ran {
    /// A piece of what `program` wrote on `stream`, or, with none, its end.
    Wrote {
        program: u64,
        stream: Stream,
        piece: Option<Vec<u8>>,
    },
    /// How `program` ended.
    Ended { program: u64, ended: Ended },
}

/// One of a program's output streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    Out,
    Err,
}

impl fmt::Display for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Out => "stdout",
            Self::Err => "stderr",
        })
    }
}

/// How a program ended: its status, if it could be had, and whether it
/// was killed for its timeout.
#[derive(Clone, Copy, Debug)]
pub struct Ended {
    status: Option<ExitStatus>,
    timed_out: bool,
}

/// What `plays.exec` and `plays.exec.start` take besides the program and
/// its arguments.
#[derive(Default)]
struct Options {
    cwd: Option<String>,
    /// Variables set, or, with no value, removed.
    env: Vec<(String, Option<String>)>,
    timeout: Option<Duration>,
    stdout: Option<Function>,
    stderr: Option<Function>,
    detach: bool,
}

impl Options {
    /// `options`, as `what` is given them: nil, or a table.
    fn read(what: &str, options: Option<Table>) -> mlua::Result<Self> {
        let Some(options) = options else {
            return Ok(Self::default());
        };
        let mut env = Vec::new();
        if let Some(variables) = options.get::<Option<Table>>("env")? {
            for variable in variables.pairs::<String, LuaValue>() {
                let (name, value) = variable?;
                let value = match value {
                    LuaValue::String(value) => Some(value.to_string_lossy()),
                    LuaValue::Boolean(false) => None,
                    _ => {
                        return Err(mlua::Error::runtime(format!(
                            "{what}'s env sets {name} to a string, or with false removes it"
                        )));
                    }
                };
                env.push((name, value));
            }
        }
        let timeout = match options.get::<Option<f64>>("timeout")? {
            Some(seconds) => Some(Duration::try_from_secs_f64(seconds).map_err(|_| {
                mlua::Error::runtime(format!("{what} can't wait {seconds} seconds"))
            })?),
            None => None,
        };
        Ok(Self {
            cwd: options.get("cwd")?,
            env,
            timeout,
            stdout: options.get("stdout")?,
            stderr: options.get("stderr")?,
            detach: options.get::<Option<bool>>("detach")?.unwrap_or(false),
        })
    }
}

/// The programs a script has going, and the part of it running.
#[derive(Default)]
pub struct Programs {
    /// Those not done, as started.
    each: RefCell<Vec<Rc<Program>>>,
    started: Cell<u64>,
    parts: Cell<u64>,
    /// Its top level, a handler, a timer, or a function output goes to.
    part: Cell<u64>,
}

/// A program a script started.
struct Program {
    id: u64,
    /// Its name, as the script gave it.
    name: String,
    /// The part of the script that started it.
    part: u64,
    detached: bool,
    out: RefCell<Output>,
    err: RefCell<Output>,
    /// Dropped, the watcher stops it.
    stop: RefCell<Option<Sender<()>>>,
    watcher: RefCell<Option<JoinHandle<Ended>>>,
    ended: Cell<Option<Ended>>,
    /// When its streams are let go, though open, once it's been killed.
    let_go: Cell<Option<Instant>>,
}

/// One of a program's streams, as the script has it.
struct Output {
    /// The function each piece goes to as it comes; with none, they're
    /// kept.
    to: Option<Function>,
    kept: Vec<u8>,
    open: bool,
}

/// A piece of a program's output for its function.
pub struct Call {
    function: Function,
    piece: Vec<u8>,
    /// Which function: whose, for which stream.
    what: String,
}

impl Call {
    /// Which function it calls: whose, for which stream.
    pub fn what(&self) -> &str {
        &self.what
    }

    /// Hand the piece to the function.
    pub fn run(self, lua: &Lua) -> mlua::Result<()> {
        self.function.call(lua.create_string(&self.piece)?)
    }
}

/// A program started, as the script has it: `:wait()` waits for it.
struct Handle<'a> {
    program: Rc<Program>,
    programs: &'a Programs,
    inbox: &'a RefCell<Inbox>,
}

impl UserData for Handle<'_> {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("wait", |lua, handle, ()| {
            handle.programs.wait(lua, handle.inbox, &handle.program)
        });
    }
}

/// `plays.exec`, under `plays`: a table that runs a program called, its
/// `start` starting one.
pub fn add<'s, 'env>(
    lua: &Lua,
    plays: &Table,
    scope: &'s Scope<'s, 'env>,
    programs: &'env Programs,
    inbox: &'env RefCell<Inbox>,
) -> mlua::Result<()> {
    let exec = lua.create_table()?;
    let runs = scope.create_function(move |lua, args| run(lua, programs, inbox, args))?;
    exec.set_metatable(Some(lua.create_table_from([("__call", runs)])?))?;
    let starts = scope.create_function(move |_, args| start(scope, programs, inbox, args))?;
    exec.set("start", starts)?;
    plays.set("exec", exec)
}

/// `plays.exec(argv, options)`: run the program `argv` names, with its
/// arguments, and wait for it: how it ended, `ok`, `code`, nil if a signal
/// ended it, and what it wrote, `stdout` and `stderr`; or nil and why it
/// couldn't run.
fn run(
    lua: &Lua,
    programs: &Programs,
    inbox: &RefCell<Inbox>,
    (_, argv, options): (LuaValue, Vec<String>, Option<Table>),
) -> mlua::Result<Answer> {
    match programs.start("plays.exec", &argv, options, inbox)? {
        Ok(program) => {
            let ended = programs.wait(lua, inbox, &program)?;
            Ok(Answer(Ok(LuaValue::Table(ended))))
        }
        Err(why) => Ok(Answer(Err(why))),
    }
}

/// `plays.exec.start(argv, options)`: start the program `argv` names, with
/// its arguments: a handle to wait for it with, or nil and why it couldn't
/// run.
fn start<'s, 'env>(
    scope: &'s Scope<'s, 'env>,
    programs: &'env Programs,
    inbox: &'env RefCell<Inbox>,
    (argv, options): (Vec<String>, Option<Table>),
) -> mlua::Result<Answer> {
    match programs.start("plays.exec.start", &argv, options, inbox)? {
        Ok(program) => {
            let handle = Handle {
                program,
                programs,
                inbox,
            };
            Ok(Answer(Ok(LuaValue::UserData(
                scope.create_userdata(handle)?,
            ))))
        }
        Err(why) => Ok(Answer(Err(why))),
    }
}

impl Programs {
    /// Start `argv` with `options`, as `what`, what it does told the
    /// script's `inbox`: why not, if it can't run.
    fn start(
        &self,
        what: &str,
        argv: &[String],
        options: Option<Table>,
        inbox: &RefCell<Inbox>,
    ) -> mlua::Result<Result<Rc<Program>, String>> {
        super::stopping::unless_caught()?;
        let options = Options::read(what, options)?;
        let Some((name, args)) = argv.split_first() else {
            return Err(mlua::Error::runtime(format!(
                "{what} takes the program to run, then its arguments"
            )));
        };
        let mut command = Command::new(name);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (variable, value) in &options.env {
            match value {
                Some(value) => command.env(variable, value),
                None => command.env_remove(variable),
            };
        }
        if let Some(cwd) = &options.cwd {
            if !Path::new(cwd).is_dir() {
                return Ok(Err(format!("can't run {name} in {cwd}: no such folder")));
            }
            command.current_dir(cwd);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(err) => return Ok(Err(format!("can't run {name}: {err}"))),
        };
        let id = self.started.get() + 1;
        self.started.set(id);
        let sender = inbox.borrow().sender();
        read(child.stdout.take(), id, Stream::Out, sender.clone());
        read(child.stderr.take(), id, Stream::Err, sender.clone());
        let (stop, stopping) = mpsc::channel();
        let deadline = options.timeout.map(after);
        let watcher = thread::spawn(move || watch(child, id, deadline, &stopping, &sender));
        let program = Rc::new(Program {
            id,
            name: name.clone(),
            part: self.part.get(),
            detached: options.detach,
            out: RefCell::new(Output::to(options.stdout)),
            err: RefCell::new(Output::to(options.stderr)),
            stop: RefCell::new(Some(stop)),
            watcher: RefCell::new(Some(watcher)),
            ended: Cell::new(None),
            let_go: Cell::new(None),
        });
        self.each.borrow_mut().push(program.clone());
        Ok(Ok(program))
    }

    /// Wait for `program` to end, its output, and other programs', going
    /// to their functions meanwhile: how it ended.
    fn wait(&self, lua: &Lua, inbox: &RefCell<Inbox>, program: &Program) -> mlua::Result<Table> {
        while !program.done() {
            // Not borrowed as a function runs, which may call Plays.
            let ran = inbox.borrow_mut().next_ran(program.let_go.get());
            match ran {
                Some(ran) => {
                    if let Some(call) = self.take(ran) {
                        call.run(lua)?;
                    }
                }
                None => {
                    // Ctrl-C, or the program let go.
                    super::stopping::unless_caught()?;
                    program.let_streams_go();
                }
            }
        }
        self.forget_done();
        program.ended(lua)
    }

    /// Take in what a program did: a piece of its output kept, or, if it
    /// has a function for it, the call that hands it over; the end of a
    /// stream, or how it ended, noted. Nothing for a program stopped or
    /// let go.
    pub fn take(&self, ran: Ran) -> Option<Call> {
        match ran {
            Ran::Wrote {
                program,
                stream,
                piece,
            } => {
                let program = self.find(program)?;
                let mut output = program.output(stream).borrow_mut();
                let Some(piece) = piece else {
                    output.open = false;
                    return None;
                };
                let Some(function) = output.to.clone() else {
                    output.kept.extend(piece);
                    return None;
                };
                let what = format!("{}'s {stream} function", program.name);
                Some(Call {
                    function,
                    piece,
                    what,
                })
            }
            Ran::Ended { program, ended } => {
                let program = self.find(program)?;
                program.ended.set(Some(ended));
                if ended.timed_out {
                    program.let_go.set(Some(Instant::now() + LET_GO));
                }
                None
            }
        }
    }

    /// The program `id`, if it's not done.
    fn find(&self, id: u64) -> Option<Rc<Program>> {
        let each = self.each.borrow();
        each.iter().find(|program| program.id == id).cloned()
    }

    /// Forget those done.
    fn forget_done(&self) {
        self.each.borrow_mut().retain(|program| !program.done());
    }

    /// Run `part`, a handler, a timer or a function output goes to, as a
    /// part of the script of its own: the programs it starts stopped as it
    /// ends, but those detached.
    pub fn part<R>(&self, part: impl FnOnce() -> R) -> R {
        let id = self.parts.get() + 1;
        self.parts.set(id);
        let was = self.part.replace(id);
        let ran = part();
        self.part.set(was);
        self.stop_part(id);
        ran
    }

    /// As the script's top level ends: the programs it started stopped,
    /// but those detached.
    pub fn top_level_over(&self) {
        self.stop_part(TOP_LEVEL);
    }

    /// Stop the programs `part` started, but those detached.
    fn stop_part(&self, part: u64) {
        let started: Vec<_> = self
            .each
            .borrow()
            .iter()
            .filter(|program| program.part == part && !program.detached)
            .cloned()
            .collect();
        for program in started {
            program.stop();
        }
        self.forget_done();
    }
}

impl Drop for Programs {
    /// As the script ends: every program it has going stopped.
    fn drop(&mut self) {
        for program in self.each.get_mut().drain(..) {
            program.stop();
        }
    }
}

impl Program {
    fn output(&self, stream: Stream) -> &RefCell<Output> {
        match stream {
            Stream::Out => &self.out,
            Stream::Err => &self.err,
        }
    }

    /// Whether it's ended, and its streams are closed or let go.
    fn done(&self) -> bool {
        self.ended.get().is_some() && !self.out.borrow().open && !self.err.borrow().open
    }

    /// No more of its output taken.
    fn let_streams_go(&self) {
        self.out.borrow_mut().open = false;
        self.err.borrow_mut().open = false;
    }

    /// Stop it, killed unless it's ended, what it writes from now on let
    /// go.
    fn stop(&self) {
        drop(self.stop.borrow_mut().take());
        let watcher = self.watcher.borrow_mut().take();
        if let Some(Ok(ended)) = watcher.map(JoinHandle::join)
            && self.ended.get().is_none()
        {
            self.ended.set(Some(ended));
        }
        self.let_streams_go();
    }

    /// How it ended, as `plays.exec` has it.
    fn ended(&self, lua: &Lua) -> mlua::Result<Table> {
        let ended = self.ended.get().expect("it's ended");
        let code = ended.status.and_then(|status| status.code());
        let table = lua.create_table()?;
        table.set("ok", code == Some(0) && !ended.timed_out)?;
        table.set("code", code)?;
        table.set("stdout", lua.create_string(&self.out.borrow().kept)?)?;
        table.set("stderr", lua.create_string(&self.err.borrow().kept)?)?;
        Ok(table)
    }
}

impl Output {
    /// A stream going to `function`, if given, else kept.
    fn to(function: Option<Function>) -> Self {
        Self {
            to: function,
            kept: Vec::new(),
            open: true,
        }
    }
}

/// Read `from`, the program `program`'s `stream`, on a thread of its own:
/// each piece told on `inbox` as it comes, and its end.
fn read(
    from: Option<impl Read + Send + 'static>,
    program: u64,
    stream: Stream,
    inbox: Sender<Came>,
) {
    let wrote = move |piece| {
        Came::Ran(Ran::Wrote {
            program,
            stream,
            piece,
        })
    };
    let Some(mut from) = from else {
        inbox.send(wrote(None)).ok();
        return;
    };
    thread::spawn(move || {
        let mut piece = vec![0; PIECE];
        loop {
            let read = match from.read(&mut piece) {
                Ok(0) => break,
                Ok(read) => read,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => {
                    log::debug!("can't read {program}'s {stream}: {err}");
                    break;
                }
            };
            if inbox.send(wrote(Some(piece[..read].to_vec()))).is_err() {
                return;
            }
        }
        inbox.send(wrote(None)).ok();
    });
}

/// Watch `child`, the program `program`, until it ends, killing it once
/// `deadline`, if any, is up, or as `stopping` says to: how it ended, told
/// on `inbox` too.
fn watch(
    mut child: Child,
    program: u64,
    deadline: Option<Instant>,
    stopping: &Receiver<()>,
    inbox: &Sender<Came>,
) -> Ended {
    let ended = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                break Ended {
                    status: Some(status),
                    timed_out: false,
                };
            }
            Ok(None) => {}
            Err(err) => {
                log::debug!("can't tell whether {program} ended: {err}");
                break kill(&mut child, false);
            }
        }
        let left = deadline.map(|deadline| deadline.saturating_duration_since(Instant::now()));
        if left.is_some_and(|left| left.is_zero()) {
            break kill(&mut child, true);
        }
        let look = left.map_or(LOOK_EVERY, |left| left.min(LOOK_EVERY));
        if !matches!(stopping.recv_timeout(look), Err(RecvTimeoutError::Timeout)) {
            break kill(&mut child, false);
        }
    };
    let ran = Ran::Ended { program, ended };
    inbox.send(Came::Ran(ran)).ok();
    ended
}

/// Kill `child`, and wait for it: how it ended, killed for its timeout
/// with `timed_out`.
fn kill(child: &mut Child, timed_out: bool) -> Ended {
    child.kill().ok();
    Ended {
        status: child.wait().ok(),
        timed_out,
    }
}

#[cfg(test)]
#[path = "tests/exec.rs"]
mod tests;
