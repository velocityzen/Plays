//! How what comes back is printed: as text, tables for lists, or as JSON
//! with `--json`; events a line each.

use std::io::{self, Write};

use serde_json::Value;

/// `value` as text: a string as it is, nothing for null, anything else as
/// compact JSON.
pub fn text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// What a command handed back: as JSON, or as text, objects and lists
/// indented.
pub fn result(out: &mut dyn Write, value: &Value, json: bool) -> io::Result<()> {
    match value {
        _ if json => pretty(out, value),
        Value::Null => Ok(()),
        Value::Array(_) | Value::Object(_) => pretty(out, value),
        other => writeln!(out, "{}", text(other)),
    }
}

/// `value` as indented JSON.
pub fn pretty(out: &mut dyn Write, value: &Value) -> io::Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    writeln!(out, "{text}")
}

/// `rows` under `header`, each column as wide as its widest, two spaces
/// apart.
pub fn table(out: &mut dyn Write, header: &[&str], rows: &[Vec<String>]) -> io::Result<()> {
    let header: Vec<String> = header.iter().map(|&title| title.to_owned()).collect();
    let lines: Vec<&Vec<String>> = std::iter::once(&header).chain(rows).collect();
    let widths: Vec<usize> = (0..header.len())
        .map(|column| {
            lines
                .iter()
                .map(|line| line.get(column).map_or(0, |cell| cell.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect();
    for line in lines {
        let mut text = String::new();
        for (cell, width) in line.iter().zip(&widths) {
            text.push_str(cell);
            text.extend(std::iter::repeat_n(' ', width - cell.chars().count() + 2));
        }
        writeln!(out, "{}", text.trim_end())?;
    }
    Ok(())
}

/// An event: as a JSON line, or its type and then its payload's fields,
/// each `key=value`.
pub fn event(out: &mut dyn Write, kind: &str, payload: &Value, json: bool) -> io::Result<()> {
    if json {
        let line = serde_json::json!({"type": kind, "payload": payload});
        writeln!(out, "{line}")?;
    } else {
        let mut line = kind.to_owned();
        if let Value::Object(fields) = payload {
            for (key, value) in fields {
                line.push_str(&format!(" {key}={}", text(value)));
            }
        }
        writeln!(out, "{line}")?;
    }
    // Each as it comes, down a pipe too.
    out.flush()
}

/// What a field is, in a table: as text, a dash for none.
pub fn cell(value: &Value) -> String {
    match value {
        Value::Null => "-".to_owned(),
        other => text(other),
    }
}

/// A star for the one in front.
pub fn front(value: &Value) -> String {
    if value["front"] == true { "*" } else { "" }.to_owned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn printed(print: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> String {
        let mut out = Vec::new();
        print(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn a_table_lines_up_its_columns() {
        let rows = [
            vec!["*".into(), "1".into(), "Living room".into()],
            vec![String::new(), "12".into(), "-".into()],
        ];
        assert_eq!(
            printed(|out| table(out, &["", "#", "LABEL"], &rows)),
            "   #   LABEL\n*  1   Living room\n   12  -\n"
        );
    }

    #[test]
    fn keys_line_up_by_the_characters_shown_not_their_bytes() {
        let rows = [
            vec!["⇧⌘O".into(), "app.open".into()],
            vec!["Space".into(), "playback.play_pause".into()],
        ];
        assert_eq!(
            printed(|out| table(out, &["KEYS", "COMMAND"], &rows)),
            "KEYS   COMMAND\n⇧⌘O    app.open\nSpace  playback.play_pause\n"
        );
    }

    #[test]
    fn results_print_as_text_or_json() {
        let print = |value: Value, json: bool| printed(|out| result(out, &value, json));
        assert_eq!(print(json!("1a2b3c4d"), false), "1a2b3c4d\n");
        assert_eq!(print(json!(0.5), false), "0.5\n");
        assert_eq!(print(Value::Null, false), "", "nothing for nothing");
        assert_eq!(print(Value::Null, true), "null\n");
        assert_eq!(print(json!("dark"), true), "\"dark\"\n");
        assert_eq!(
            print(json!({"window": "1a"}), false),
            "{\n  \"window\": \"1a\"\n}\n"
        );
    }

    #[test]
    fn events_print_a_line_each() {
        let payload = json!({"window_id": "1a", "tile_id": "2b", "position": 1.5});
        assert_eq!(
            printed(|out| event(out, "paused", &payload, false)),
            "paused window_id=1a tile_id=2b position=1.5\n"
        );
        assert_eq!(
            printed(|out| event(out, "paused", &payload, true)),
            "{\"type\":\"paused\",\"payload\":{\"window_id\":\"1a\",\"tile_id\":\"2b\",\"position\":1.5}}\n"
        );
    }
}
