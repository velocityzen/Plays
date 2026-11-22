//! `plays actions`: every command, from the registry this program has, so
//! Plays needn't be running. As a table of ids, titles and keys; as JSON,
//! each described; or, with `--markdown`, the command reference kept in
//! `docs/commands.md`.

use std::fmt::Write as _;
use std::io::{self, Write};

use serde_json::Value;

use super::show;
use crate::api::args::{Arg, Kind};
use crate::api::{self, Scope, Spec};
use crate::hotkeys;

/// Print every command: as a table, JSON, or the command reference.
pub fn actions(out: &mut dyn Write, json: bool, markdown: bool) -> io::Result<()> {
    if markdown {
        return write!(out, "{}", reference());
    }
    if json {
        let described: Value = api::all().map(Spec::describe).collect();
        return show::pretty(out, &described);
    }
    let rows: Vec<Vec<String>> = api::all()
        .map(|spec| vec![spec.id.to_owned(), spec.title.to_owned(), keys(spec)])
        .collect();
    show::table(out, &["ID", "TITLE", "KEYS"], &rows)
}

/// The keys a command starts out with, as menus show them.
fn keys(spec: &Spec) -> String {
    let shown: Vec<String> = spec.keys.iter().map(|key| hotkeys::display(key)).collect();
    shown.join(", ")
}

/// The command reference, in Markdown: each namespace's commands, with
/// what they act on, their keys, arguments and what they hand back.
pub fn reference() -> String {
    let mut text = String::from(
        "# Commands\n\n\
         Every command Plays has, by namespace, as `plays actions --markdown` writes \
         them. A terminal runs one with `plays do ID [KEY=VALUE…]`, naming the window \
         or tile it acts on with `-w` and `-t`; keys run those that have them, and \
         Settings → Hotkeys changes those. The keys here are macOS's: elsewhere Ctrl \
         stands for ⌘, and Alt for ⌥.\n",
    );
    let mut namespace = "";
    for spec in api::all() {
        if spec.namespace() != namespace {
            namespace = spec.namespace();
            writeln!(text, "\n## {namespace}").ok();
        }
        command(&mut text, spec);
    }
    text
}

/// One command, in the reference.
fn command(text: &mut String, spec: &Spec) {
    writeln!(
        text,
        "\n### {}\n\n`{}`: {}\n",
        spec.title, spec.id, spec.description
    )
    .ok();
    writeln!(text, "- Acts on {}.", acts_on(spec.scope)).ok();
    if !spec.keys.is_empty() {
        writeln!(text, "- Keys: {}", keys(spec)).ok();
    }
    if !spec.args.is_empty() {
        text.push_str("- Arguments:\n");
        for arg in spec.args {
            writeln!(
                text,
                "  - `{}` ({}): {}",
                arg.name,
                kind(arg),
                arg.description
            )
            .ok();
        }
    }
    if let Some(returns) = spec.returns {
        writeln!(text, "- Hands back: {returns}").ok();
    }
}

fn acts_on(scope: Scope) -> &'static str {
    match scope {
        Scope::App => "the app",
        Scope::AnyWindow => "a window, Settings and About too",
        Scope::Window => "a player window",
        Scope::Tiles => "every tile in a window",
        Scope::Tile => "a tile",
    }
}

/// What an argument takes, and whether it must be given.
fn kind(arg: &Arg) -> String {
    let kind = match &arg.kind {
        Kind::Integer { min } => format!("a whole number, at least {min}"),
        Kind::Number {
            min,
            max: Some(max),
        } => format!("a number from {min} to {max}"),
        Kind::Number { min, max: None } => format!("a number, at least {min}"),
        Kind::Text => "text".to_owned(),
        Kind::Texts => "a list of text".to_owned(),
        Kind::Choice(choices) => format!("one of {}", choices().join(", ")),
        Kind::Any => "any value".to_owned(),
    };
    if arg.required {
        kind
    } else {
        format!("{kind}, optional")
    }
}
