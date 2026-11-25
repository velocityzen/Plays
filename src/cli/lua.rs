//! The Lua scripts `plays do` runs: a file, `-e` code or stdin, run here,
//! a client of the Plays running as the command line is. `plays` holds
//! what a script is given, calls Plays' commands, waits (`waiting`),
//! registers handlers (`handlers`), starts timers (`timers`), runs
//! programs (`exec`), lists folders (`folders`) and reads and writes JSON
//! (`json`); what the script returns is printed as JSON, and then, if it
//! has handlers or timers, it runs on (`listening`). What it waits for
//! comes to its inbox (`inbox`).
//!
//! JSON and Lua: null is nil, and `plays.null` sends one; an object or a
//! list is a table, a list from Plays staying one, so a table is a list if
//! it came as one or has items from 1, else an object; integers stay
//! integers and other numbers floats, both ways.

mod exec;
mod folders;
mod handlers;
mod inbox;
mod json;
mod listening;
mod stopping;
mod timers;
mod waiting;

use std::cell::{Cell, RefCell};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use mlua::serde::SerializeOptions;
use mlua::{
    AnyUserData, Function, HookTriggers, IntoLuaMulti, Lua, LuaSerdeExt, MultiValue, Table,
    Value as LuaValue,
};
use serde_json::Value;

use self::exec::Programs;
use self::handlers::Handlers;
use self::inbox::Inbox;
use self::listening::Listener;
pub use self::listening::Listening;
use self::stopping::OnStop;
use self::timers::Timers;
use super::{Done, Session, Stop, show};
use crate::api::socket::Heard;
use crate::api::socket::client::Failure;
use crate::api::{self, Error, Scope, Target};

/// What a script stopped by `plays.stop` fails with, step after step.
const STOPPED: &str = "stopped";

/// A script to run: its code, its name in what an error says, and the
/// folder `require` looks in first.
pub struct Script {
    code: Vec<u8>,
    name: String,
    folder: Option<PathBuf>,
}

/// Whether `name`, given to `plays do`, is a script: a file when it ends in
/// .lua or has a slash, stdin when `-`; else a command's id.
pub fn is_script(name: &str) -> bool {
    name == "-" || name.ends_with(".lua") || name.contains('/')
}

impl Script {
    /// `code`, given with `-e`.
    pub fn inline(code: String) -> Self {
        Self {
            code: code.into_bytes(),
            name: "=(command line)".into(),
            folder: None,
        }
    }

    /// The script `name` names: read from `input` for `-`, else the file.
    pub fn read(name: &str, input: &mut dyn Read) -> Result<Self, Stop> {
        if name == "-" {
            let mut code = Vec::new();
            input
                .read_to_end(&mut code)
                .map_err(|err| Stop::Script(format!("can't read stdin: {err}")))?;
            return Ok(Self {
                code,
                name: "=stdin".into(),
                folder: None,
            });
        }
        let path = Path::new(name);
        let code = std::fs::read(path).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => Stop::from(Error::not_found("script", name)),
            _ => Stop::Script(format!("can't read {name}: {err}")),
        })?;
        Ok(Self {
            code,
            name: format!("@{name}"),
            folder: path.parent().map(Path::to_owned),
        })
    }
}

/// Run `script` with `args`, on the window and block `-w` and `-b` name,
/// else the window in front and the block keys go to in it: print what it
/// returns, then run on as `listening` says, if it has handlers or timers.
pub fn run(session: &mut Session, script: Script, args: Value, listening: Listening) -> Done {
    let here = whereabouts(session)?;
    let lua = Lua::new();
    let failed = |err: mlua::Error| Stop::Script(message(&err));
    set_up(&lua, script.folder.as_deref()).map_err(failed)?;
    let tostring: Function = lua.globals().get("tostring").map_err(failed)?;
    let stopped = Cell::new(None);
    let window = here
        .window
        .clone()
        .filter(|_| session.options.window.is_some());
    let heard = window.clone().map_or(Heard::All, Heard::Window);
    let inbox = RefCell::new(Inbox::new(session.socket.clone(), heard));
    let handlers = Handlers::default();
    let timers = Timers::default();
    let programs = Programs::default();
    let on_stop = OnStop::default();
    let session = RefCell::new(session);
    let here = RefCell::new(here);
    let ran = lua.scope(|scope| {
        let plays = lua.create_table()?;
        plays.set("args", to_lua(&lua, &args)?)?;
        plays.set("window_id", here.borrow().window.clone())?;
        plays.set("block_id", here.borrow().block.clone())?;
        plays.set("null", lua.null())?;
        let by_id = scope.create_function(|lua, (id, args): (String, LuaValue)| {
            call(lua, &session, &inbox, &id, args, &here.borrow())
        })?;
        plays.set("call", by_id)?;
        let command = scope.create_function(|lua, (node, args): (Table, LuaValue)| {
            let id: String = node
                .metatable()
                .ok_or_else(|| mlua::Error::runtime("not a command"))?
                .raw_get("id")?;
            call(lua, &session, &inbox, &id, args, &here.borrow())
        })?;
        add_commands(&lua, &plays, &command)?;
        let stops =
            scope.create_function(|lua, status: Option<i64>| stop(lua, status, &stopped))?;
        plays.set("stop", stops)?;
        waiting::add(&lua, &plays, scope, &inbox)?;
        exec::add(&lua, &plays, scope, &programs, &inbox)?;
        json::add(&lua, &plays)?;
        folders::add(&lua, &plays)?;
        plays.set("on", scope.create_function(|_, args| handlers.on(args))?)?;
        let stops = scope.create_function(|lua, function: Option<Function>| {
            on_stop.set(lua, function, inbox.borrow().sender())
        })?;
        plays.set("on_stop", stops)?;
        let (timers, stopped, session) = (&timers, &stopped, &session);
        let timer = |every| {
            scope
                .create_function(move |lua, args| timer(lua, timers, args, every, stopped, session))
        };
        plays.set("after", timer(false)?)?;
        plays.set("every", timer(true)?)?;
        add_output(&lua, &plays, scope, session, &tostring)?;
        lua.globals().set("plays", plays)?;
        let script_here = here.borrow().clone();
        let ran = lua
            .load(&script.code[..])
            .set_name(&script.name)
            .call::<MultiValue>(())
            .map(|returned| {
                programs.top_level_over();
                let say = |line: &str| writeln!(session.borrow_mut().err, "{line}");
                let listener = Listener {
                    lua: &lua,
                    handlers: &handlers,
                    timers,
                    programs: &programs,
                    inbox: &inbox,
                    here: &here,
                    window: window.as_deref(),
                    stopped,
                    say: &say,
                    listening,
                };
                let printed = print_returned(&mut session.borrow_mut(), &lua, returned);
                printed.and_then(|()| listening::listen(&listener))
            });
        let interrupted = stopping::caught();
        let stopping = Stopping {
            lua: &lua,
            inbox: &inbox,
            here: &here,
            programs: &programs,
        };
        let on_stop_ran = stopping.run(&on_stop, script_here);
        Ok((ran, on_stop_ran, interrupted))
    });
    let (ran, on_stop_ran, interrupted) = ran.map_err(failed)?;
    let err = &mut *session.borrow_mut().err;
    ended(ran, on_stop_ran, interrupted, stopped.get(), err)
}

/// What a script's `plays.on_stop` function runs with.
struct Stopping<'a> {
    lua: &'a Lua,
    inbox: &'a RefCell<Inbox>,
    here: &'a RefCell<Target>,
    programs: &'a Programs,
}

impl Stopping<'_> {
    /// As the script ends, however it ends but as Plays goes: its
    /// `plays.on_stop` function run, if it gave one, once, its calls on
    /// the script's window and block; what that came to.
    fn run(&self, on_stop: &OnStop, script: Target) -> mlua::Result<()> {
        let Some(function) = on_stop.take() else {
            return Ok(());
        };
        if self.inbox.borrow().plays_gone() {
            return Ok(());
        }
        stopping::calm(self.lua);
        self.here.replace(script);
        self.programs.part(|| function.call::<()>(()))
    }
}

/// How a script that `ran` ended: as `plays.stop` said, if it `stopped`;
/// else failed, if its `plays.on_stop` function did; else as Ctrl-C ends
/// one, if it was `interrupted`; else as it ran. Its function's failure
/// is said on `err` where the script's own is what it ends with.
fn ended(
    ran: mlua::Result<Done>,
    on_stop: mlua::Result<()>,
    interrupted: bool,
    stopped: Option<u8>,
    err: &mut dyn Write,
) -> Done {
    if let Some(status) = stopped {
        return Err(Stop::Exit(status));
    }
    if let Err(failure) = on_stop {
        let why = format!("plays.on_stop's function failed: {}", message(&failure));
        if interrupted || matches!(ran, Ok(Ok(()))) {
            return Err(Stop::Script(why));
        }
        writeln!(err, "plays: {why}")?;
    }
    if interrupted {
        return Err(Stop::Exit(stopping::INTERRUPTED_STATUS));
    }
    ran.map_err(|err| Stop::Script(message(&err)))?
}

/// The ids of the window and block `-w` and `-b` name, else of the window
/// in front and the block keys go to in it: none where there's none.
fn whereabouts(session: &mut Session) -> Result<Target, Stop> {
    let named = session.target();
    match session.call_on("block.inspect", Value::Null, named.clone()) {
        Ok(block) => {
            return Ok(Target {
                window: Some(show::text(&block["window"])),
                block: Some(show::text(&block["id"])),
            });
        }
        Err(Failure::Command(err)) if err.is_not_found() && named.block.is_none() => {}
        Err(failure) => return Err(failure.into()),
    }
    let window = Target {
        window: named.window.clone(),
        block: None,
    };
    match session.call_on("window.inspect", Value::Null, window) {
        Ok(window) => Ok(Target {
            window: Some(show::text(&window["id"])),
            block: None,
        }),
        Err(Failure::Command(err)) if err.is_not_found() && named.window.is_none() => {
            Ok(Target::default())
        }
        Err(failure) => Err(failure.into()),
    }
}

/// The standard library as a script has it: without `io.popen` and
/// `os.execute`, and with `require` looking in the script's folder first.
fn set_up(lua: &Lua, folder: Option<&Path>) -> mlua::Result<()> {
    let globals = lua.globals();
    globals.get::<Table>("io")?.set("popen", LuaValue::Nil)?;
    globals.get::<Table>("os")?.set("execute", LuaValue::Nil)?;
    if let Some(folder) = folder.filter(|folder| !folder.as_os_str().is_empty()) {
        let package: Table = globals.get("package")?;
        let path: String = package.get("path")?;
        let folder = folder.display();
        package.set("path", format!("{folder}/?.lua;{folder}/?/init.lua;{path}"))?;
    }
    Ok(())
}

/// Each command, under `plays` by its id, as `plays.playback.seek`: a
/// table, as a namespace may be within it, that runs it when called.
fn add_commands(lua: &Lua, plays: &Table, command: &Function) -> mlua::Result<()> {
    for spec in api::all() {
        let mut node = plays.clone();
        for part in spec.id.split('.') {
            node = match node.raw_get::<Option<Table>>(part)? {
                Some(within) => within,
                None => {
                    let within = lua.create_table()?;
                    node.raw_set(part, &within)?;
                    within
                }
            };
        }
        let runs = lua.create_table()?;
        runs.raw_set("__call", command)?;
        runs.raw_set("id", spec.id)?;
        node.set_metatable(Some(runs))?;
    }
    Ok(())
}

/// What a call, a wait, a program or JSON hands a script: the value alone,
/// or nil and why not, as Lua's own functions do.
pub struct Answer(pub Result<LuaValue, String>);

impl IntoLuaMulti for Answer {
    fn into_lua_multi(self, lua: &Lua) -> mlua::Result<MultiValue> {
        match self.0 {
            Ok(value) => value.into_lua_multi(lua),
            Err(why) => (LuaValue::Nil, why).into_lua_multi(lua),
        }
    }
}

/// Run the command `id` with `args`, a table or nil: what it hands back,
/// else nil and why not. A wait after it hears what it does, and nothing
/// from before. Once Ctrl-C is caught, none is made, and one made
/// meanwhile fails.
fn call(
    lua: &Lua,
    session: &RefCell<&mut Session>,
    inbox: &RefCell<Inbox>,
    id: &str,
    args: LuaValue,
    here: &Target,
) -> mlua::Result<Answer> {
    stopping::unless_caught()?;
    let mut args: Value = lua.from_value(args)?;
    let target = target(id, &mut args, here);
    inbox.borrow_mut().hear_from_now();
    let called = session.borrow_mut().call_on(id, args, target);
    stopping::unless_caught()?;
    match called {
        Ok(value) => Ok(Answer(Ok(to_lua(lua, &value)?))),
        Err(failure) => Ok(Answer(Err(failure.to_string()))),
    }
}

/// What the command `id` acts on: the window and block `window_id` and
/// `block_id` name, taken out of `args`; else the script's, as far as the
/// command acts on a window or a block.
fn target(id: &str, args: &mut Value, here: &Target) -> Target {
    let mut named = |key| {
        args.as_object_mut()
            .and_then(|args| args.remove(key))
            .map(|name| show::text(&name))
    };
    let (window, block) = (named("window_id"), named("block_id"));
    if window.is_some() || block.is_some() {
        return Target { window, block };
    }
    match api::find(id).map(|spec| spec.scope) {
        Some(Scope::Block) => here.clone(),
        Some(Scope::AnyWindow | Scope::Window | Scope::Blocks) => Target {
            window: here.window.clone(),
            block: None,
        },
        _ => Target::default(),
    }
}

/// `plays.after` or, with `every`, `plays.every`: a timer, unless the
/// script has as many as it may, which stops it, exit status 1.
fn timer(
    lua: &Lua,
    timers: &Timers,
    args: (f64, Function),
    every: bool,
    stopped: &Cell<Option<u8>>,
    session: &RefCell<&mut Session>,
) -> mlua::Result<AnyUserData> {
    if timers.count() >= timers::MOST {
        let line = format!("plays: a script has at most {} timers\n", timers::MOST);
        write_all(&mut *session.borrow_mut().err, line.as_bytes())?;
        stop(lua, Some(1), stopped)?;
    }
    timers.add(lua, args, every)
}

/// End the script now with exit status `status`, 0 if none: an error raised
/// again at each step on, so no `pcall` holds it.
fn stop(lua: &Lua, status: Option<i64>, stopped: &Cell<Option<u8>>) -> mlua::Result<()> {
    let status = u8::try_from(status.unwrap_or(0))
        .map_err(|_| mlua::Error::runtime("plays.stop takes an exit status from 0 to 255"))?;
    stopped.set(Some(status));
    lua.set_hook(HookTriggers::new().every_nth_instruction(1), |_, _| {
        Err(mlua::Error::runtime(STOPPED))
    })?;
    Err(mlua::Error::runtime(STOPPED))
}

/// `print` and `plays.log`: a line on the session's stdout, or stderr,
/// added for as long as `scope` is.
fn add_output<'s, 'env>(
    lua: &Lua,
    plays: &Table,
    scope: &'s mlua::Scope<'s, 'env>,
    session: &'env RefCell<&mut Session>,
    tostring: &'env Function,
) -> mlua::Result<()> {
    let log = scope.create_function(move |_, values: MultiValue| {
        let line = line(tostring, values)?;
        write_all(&mut *session.borrow_mut().err, &line)
    })?;
    plays.set("log", log)?;
    let print = scope.create_function(move |_, values: MultiValue| {
        let line = line(tostring, values)?;
        write_all(&mut *session.borrow_mut().out, &line)
    })?;
    lua.globals().set("print", print)
}

/// `values` on a line of their own, as Lua's `print` writes them: each as
/// `tostring` has it, with tabs between.
fn line(tostring: &Function, values: MultiValue) -> mlua::Result<Vec<u8>> {
    let mut line = Vec::new();
    for (at, value) in values.into_iter().enumerate() {
        if at > 0 {
            line.push(b'\t');
        }
        let text: mlua::LuaString = tostring.call(value)?;
        line.extend_from_slice(&text.as_bytes());
    }
    line.push(b'\n');
    Ok(line)
}

fn write_all(to: &mut dyn io::Write, line: &[u8]) -> mlua::Result<()> {
    to.write_all(line).map_err(mlua::Error::external)
}

/// `value` in Lua: null as nil.
fn to_lua(lua: &Lua, value: &Value) -> mlua::Result<LuaValue> {
    let options = SerializeOptions::new()
        .serialize_none_to_null(false)
        .serialize_unit_to_null(false);
    lua.to_value_with(value, options)
}

/// Print what the script returned as JSON, indented with `--json`: nothing
/// for nothing.
fn print_returned(session: &mut Session, lua: &Lua, returned: MultiValue) -> Done {
    let Some(value) = returned.into_iter().next().filter(|value| !value.is_nil()) else {
        return Ok(());
    };
    let value: Value = lua
        .from_value(value)
        .map_err(|err| Stop::Script(format!("what it returned isn't JSON: {}", message(&err))))?;
    if session.options.json {
        show::pretty(session.out, &value)?;
    } else {
        writeln!(session.out, "{value}")?;
    }
    Ok(())
}

/// Why a script failed, as Lua says it, without the calls it was in.
fn message(err: &mlua::Error) -> String {
    match err {
        mlua::Error::RuntimeError(message) | mlua::Error::SyntaxError { message, .. } => {
            let (message, _) = message
                .split_once("\nstack traceback:")
                .unwrap_or((message, ""));
            message.to_owned()
        }
        mlua::Error::CallbackError { cause, .. } => message(cause),
        err => err.to_string(),
    }
}

#[cfg(test)]
#[path = "tests/lua.rs"]
mod tests;

#[cfg(test)]
#[path = "lua/tests/guides.rs"]
mod guides;
