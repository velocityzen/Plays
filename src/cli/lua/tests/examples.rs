//! The example scripts in `lua/`: each is Lua, says what it is on its
//! first line, and calls only commands Plays has, with arguments they
//! take.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use mlua::Lua;

use crate::api::{self, Spec};

/// What any command takes besides its arguments: the window or block it
/// acts on.
const TARGETS: [&str; 2] = ["window_id", "block_id"];

/// The scripts in `lua/`, by name: each's name and code.
fn scripts() -> Vec<(String, String)> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("lua");
    let mut scripts: Vec<(String, String)> = fs::read_dir(&folder)
        .expect("lua/")
        .map(|entry| entry.expect("an entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "lua"))
        .map(|path| {
            let name = path.file_name().expect("a name").to_string_lossy();
            (
                name.into_owned(),
                fs::read_to_string(&path).expect("a script"),
            )
        })
        .collect();
    scripts.sort();
    scripts
}

/// A command `code` calls: its id, as `plays.block.playback.seek{…}`
/// calls `block.playback.seek`, and the arguments a table gives it.
struct Call<'a> {
    id: &'a str,
    args: Vec<&'a str>,
}

/// The commands `code` calls.
fn calls(code: &str) -> Vec<Call<'_>> {
    let roots: BTreeSet<&str> = api::all()
        .filter_map(|spec| spec.id.split_once('.').map(|(root, _)| root))
        .collect();
    let named = |c: char| c.is_ascii_alphanumeric() || c == '_';
    code.match_indices("plays.")
        .filter(|&(at, _)| !code[..at].ends_with(named))
        .filter_map(|(at, found)| {
            let rest = &code[at + found.len()..];
            let end = rest.find(|c| !named(c) && c != '.').unwrap_or(rest.len());
            let (id, after) = rest.split_at(end);
            let (root, _) = id.split_once('.')?;
            if !roots.contains(root) || !after.starts_with(['{', '(']) {
                return None;
            }
            let args = after.strip_prefix('{').map(table_keys).unwrap_or_default();
            Some(Call { id, args })
        })
        .collect()
}

/// The keys of the table `table` starts, its `{` gone: the names before
/// an `=` in it, not in a table or string within it.
fn table_keys(table: &str) -> Vec<&str> {
    let mut keys = Vec::new();
    let (mut depth, mut quote, mut name) = (0_usize, None, None);
    for (at, c) in table.char_indices() {
        if let Some(open) = quote {
            if c == open {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '{' => depth += 1,
            '}' if depth == 0 => break,
            '}' => depth -= 1,
            '=' if depth == 0 && !table[at + 1..].starts_with('=') => {
                if let Some(start) = name.take() {
                    keys.push(table[start..at].trim());
                }
            }
            _ if c.is_ascii_alphanumeric() || c == '_' => {
                name.get_or_insert(at);
                continue;
            }
            _ if c.is_whitespace() => continue,
            _ => {}
        }
        name = None;
    }
    keys
}

/// What `spec` takes, the window or block too.
fn takes(spec: &Spec) -> Vec<&str> {
    spec.args
        .iter()
        .map(|arg| arg.name)
        .chain(TARGETS)
        .collect()
}

#[test]
fn every_example_is_lua_says_what_it_is_and_calls_only_commands_plays_has() {
    let lua = Lua::new();
    let scripts = scripts();
    assert!(!scripts.is_empty(), "lua/ has examples");
    for (name, code) in &scripts {
        let compiled = lua
            .load(code.as_str())
            .set_name(name.as_str())
            .into_function();
        assert!(compiled.is_ok(), "{name}: {compiled:?}");
        assert!(
            code.starts_with(&format!("-- {name}: ")),
            "{name} says what it is first"
        );
        let calls = calls(code);
        assert!(!calls.is_empty(), "{name} calls commands");
        for Call { id, args } in calls {
            let spec = api::find(id).unwrap_or_else(|| panic!("{name} calls {id}: no command"));
            for arg in args {
                assert!(
                    takes(spec).contains(&arg),
                    "{name} gives {id} {arg}, which it doesn't take"
                );
            }
        }
    }
}

#[test]
fn the_calls_in_a_script_are_found_with_their_arguments() {
    let code = r#"
        plays.log("displays.window.new()")
        local w = must(plays.window.new()).window
        plays.app.open{window_id = w, urls = {folder, "a=b"}, kind = "video", n = x == 1}
        plays.block.playback.seek{ block_id = id, time = 0 }
        local listed = plays.block.ls{window_id = w}
        plays.wait("block.created", {window_id = w}, 10)
    "#;
    let found: Vec<(&str, Vec<&str>)> = calls(code)
        .into_iter()
        .map(|call| (call.id, call.args))
        .collect();
    assert_eq!(
        found,
        [
            ("window.new", vec![]),
            ("app.open", vec!["window_id", "urls", "kind", "n"]),
            ("block.playback.seek", vec!["block_id", "time"]),
            ("block.ls", vec!["window_id"]),
        ]
    );
}
