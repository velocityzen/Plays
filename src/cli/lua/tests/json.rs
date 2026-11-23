//! `plays.json`: Lua values to JSON text and back, as calls convert them.

use mlua::Lua;
use serde_json::{Value, json};

use super::add;

/// A Lua with `plays.json`.
fn lua() -> Lua {
    let lua = Lua::new();
    let plays = lua.create_table().unwrap();
    add(&lua, &plays).unwrap();
    lua.globals().set("plays", plays).unwrap();
    lua
}

#[test]
fn a_value_is_json_text_indented_if_asked() {
    let lua = lua();
    let text: String = lua
        .load(r#"return plays.json.encode({name = "work", panes = {1, 2.5}, none = {}})"#)
        .eval()
        .unwrap();
    let read: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(read, json!({"name": "work", "panes": [1, 2.5], "none": {}}));
    assert!(!text.contains('\n'), "{text}");

    let indented: String = lua
        .load("return plays.json.encode({panes = {1}}, {indent = true})")
        .eval()
        .unwrap();
    assert_eq!(indented, "{\n  \"panes\": [\n    1\n  ]\n}");

    let (none, why): (Option<String>, String) = lua
        .load("return plays.json.encode({print})")
        .eval()
        .unwrap();
    assert_eq!(none, None);
    assert!(why.contains("function"), "{why}");
}

#[test]
fn json_text_is_a_value_numbers_lists_and_objects_as_they_were() {
    let lua = lua();
    let read: (String, String, String, bool, String) = lua
        .load(
            r#"
            local value = plays.json.decode('{"list": [1, 2.5, "x"], "nothing": null, "empty": []}')
            return math.type(value.list[1]), math.type(value.list[2]), value.list[3],
              value.nothing == nil, plays.json.encode(value.empty)
            "#,
        )
        .eval()
        .unwrap();
    assert_eq!(
        read,
        (
            "integer".into(),
            "float".into(),
            "x".into(),
            true,
            "[]".into()
        )
    );

    let (none, why): (Option<String>, String) =
        lua.load("return plays.json.decode('{')").eval().unwrap();
    assert_eq!(none, None);
    assert!(why.contains("EOF"), "{why}");
}
