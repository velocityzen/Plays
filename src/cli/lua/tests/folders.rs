//! `plays.ls`: what's in a folder, as Lua sees it.

use std::fs;
use std::os::unix::fs::symlink;

use mlua::{Lua, LuaSerdeExt, Value as LuaValue};
use serde_json::{Value, json};

use super::add;
use crate::files::test_folder;

/// A Lua with `plays.ls`.
fn lua() -> Lua {
    let lua = Lua::new();
    let plays = lua.create_table().unwrap();
    add(&lua, &plays).unwrap();
    lua.globals().set("plays", plays).unwrap();
    lua
}

#[test]
fn a_folder_lists_by_name_each_a_file_a_folder_or_other() {
    let folder = test_folder("lua-ls");
    fs::create_dir_all(folder.join("b-folder")).unwrap();
    fs::write(folder.join("c.mkv"), b"").unwrap();
    fs::write(folder.join(".hidden"), b"").unwrap();
    symlink(folder.join("c.mkv"), folder.join("a-link.mkv")).unwrap();
    symlink(folder.join("gone"), folder.join("d-broken")).unwrap();
    let lua = lua();
    let listed: LuaValue = lua
        .load(format!(
            "return plays.ls({:?})",
            folder.display().to_string()
        ))
        .eval()
        .unwrap();
    let listed: Value = lua.from_value(listed).unwrap();
    let entry = |name: &str, kind: &str| json!({"name": name, "path": folder.join(name).display().to_string(), "type": kind});
    assert_eq!(
        listed,
        json!([
            entry(".hidden", "file"),
            entry("a-link.mkv", "file"),
            entry("b-folder", "folder"),
            entry("c.mkv", "file"),
            entry("d-broken", "other"),
        ])
    );

    let (none, why): (Option<LuaValue>, String) = lua
        .load(format!(
            "return plays.ls({:?})",
            folder.join("gone").display().to_string()
        ))
        .eval()
        .unwrap();
    assert!(none.is_none());
    assert!(
        why.starts_with(&folder.join("gone").display().to_string()),
        "{why}"
    );
}

#[test]
fn an_empty_folder_is_an_empty_list() {
    let folder = test_folder("lua-ls-empty");
    fs::create_dir_all(&folder).unwrap();
    let lua = lua();
    let listed: LuaValue = lua
        .load(format!(
            "return plays.ls({:?})",
            folder.display().to_string()
        ))
        .eval()
        .unwrap();
    assert_eq!(lua.from_value::<Value>(listed).unwrap(), json!([]));
}
