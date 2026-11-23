//! The Lua scripts `plays do` runs: a file, `-e` code or stdin, run here,
//! a client of the Plays running as the command line is. `plays` holds what a script is given and calls Plays'
//! commands; what the script returns is printed as JSON.
//!
//! JSON and Lua: null is nil, and `plays.null` sends one; an object or a
//! list is a table, a list from Plays staying one, so a table is a list if
//! it came as one or has items from 1, else an object; integers stay
//! integers and other numbers floats, both ways.

use std::cell::{Cell, RefCell};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use mlua::serde::SerializeOptions;
use mlua::{Function, HookTriggers, Lua, LuaSerdeExt, MultiValue, Table, Value as LuaValue};
use serde_json::Value;

use super::{Done, Session, Stop, show};
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
/// returns.
pub fn run(session: &mut Session, script: Script, args: Value) -> Done {
    let here = whereabouts(session)?;
    let lua = Lua::new();
    let failed = |err: mlua::Error| Stop::Script(message(&err));
    set_up(&lua, script.folder.as_deref()).map_err(failed)?;
    let tostring: Function = lua.globals().get("tostring").map_err(failed)?;
    let stopped = Cell::new(None);
    let session = RefCell::new(session);
    let ran = lua.scope(|scope| {
        let plays = lua.create_table()?;
        plays.set("args", to_lua(&lua, &args)?)?;
        plays.set("window_id", here.window.clone())?;
        plays.set("block_id", here.block.clone())?;
        plays.set("null", lua.null())?;
        let by_id = scope.create_function(|lua, (id, args): (String, LuaValue)| {
            call(lua, &session, &id, args, &here)
        })?;
        plays.set("call", by_id)?;
        let command = scope.create_function(|lua, (node, args): (Table, LuaValue)| {
            let id: String = node
                .metatable()
                .ok_or_else(|| mlua::Error::runtime("not a command"))?
                .raw_get("id")?;
            call(lua, &session, &id, args, &here)
        })?;
        add_commands(&lua, &plays, &command)?;
        let stops =
            scope.create_function(|lua, status: Option<i64>| stop(lua, status, &stopped))?;
        plays.set("stop", stops)?;
        let log = scope.create_function(|_, values: MultiValue| {
            let line = line(&tostring, values)?;
            write_all(&mut *session.borrow_mut().err, &line)
        })?;
        plays.set("log", log)?;
        let print = scope.create_function(|_, values: MultiValue| {
            let line = line(&tostring, values)?;
            write_all(&mut *session.borrow_mut().out, &line)
        })?;
        lua.globals().set("print", print)?;
        lua.globals().set("plays", plays)?;
        lua.load(&script.code[..])
            .set_name(&script.name)
            .call::<MultiValue>(())
    });
    if let Some(status) = stopped.get() {
        return Err(Stop::Exit(status));
    }
    let returned = ran.map_err(failed)?;
    print_returned(session.into_inner(), &lua, returned)
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

/// Run the command `id` with `args`, a table or nil: what it hands back,
/// else nil and why not.
fn call(
    lua: &Lua,
    session: &RefCell<&mut Session>,
    id: &str,
    args: LuaValue,
    here: &Target,
) -> mlua::Result<(LuaValue, Option<String>)> {
    let mut args: Value = lua.from_value(args)?;
    let target = target(id, &mut args, here);
    let called = session.borrow_mut().call_on(id, args, target);
    match called {
        Ok(value) => Ok((to_lua(lua, &value)?, None)),
        Err(failure) => Ok((LuaValue::Nil, Some(failure.to_string()))),
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
