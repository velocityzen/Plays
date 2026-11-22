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
#[path = "tests/show.rs"]
mod tests;
