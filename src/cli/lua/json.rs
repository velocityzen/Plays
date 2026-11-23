//! `plays.json`: JSON text from a Lua value, and a Lua value from JSON
//! text, converting as calls do.

use mlua::{Lua, LuaSerdeExt, LuaString, Table, Value as LuaValue};
use serde_json::Value;

use super::{message, to_lua};

/// `plays.json`, under `plays`.
pub fn add(lua: &Lua, plays: &Table) -> mlua::Result<()> {
    let json = lua.create_table()?;
    json.set("encode", lua.create_function(encode)?)?;
    json.set("decode", lua.create_function(decode)?)?;
    plays.set("json", json)
}

/// `plays.json.encode(value, options)`: `value` as JSON text, indented by
/// two spaces with `{indent = true}`; or nil and why it can't be.
fn encode(
    lua: &Lua,
    (value, options): (LuaValue, Option<Table>),
) -> mlua::Result<(Option<String>, Option<String>)> {
    let indent = match options {
        Some(options) => options.get::<Option<bool>>("indent")?.unwrap_or(false),
        None => false,
    };
    let value: Value = match lua.from_value(value) {
        Ok(value) => value,
        Err(err) => return Ok((None, Some(message(&err)))),
    };
    let text = if indent {
        serde_json::to_string_pretty(&value)
    } else {
        serde_json::to_string(&value)
    };
    Ok(match text {
        Ok(text) => (Some(text), None),
        Err(err) => (None, Some(err.to_string())),
    })
}

/// `plays.json.decode(text)`: the value JSON `text` holds, or nil and why
/// not.
fn decode(lua: &Lua, text: LuaString) -> mlua::Result<(LuaValue, Option<String>)> {
    match serde_json::from_slice::<Value>(&text.as_bytes()) {
        Ok(value) => Ok((to_lua(lua, &value)?, None)),
        Err(err) => Ok((LuaValue::Nil, Some(err.to_string()))),
    }
}

#[cfg(test)]
#[path = "tests/json.rs"]
mod tests;
