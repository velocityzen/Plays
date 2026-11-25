//! `plays actions`: every command, or those a search finds, from the
//! registry this program has, so Plays needn't be running. As a table of
//! ids, titles and keys; as JSON, each described; or, with `--markdown`,
//! the command reference kept in `docs/commands.md`, with every event Plays
//! tells and what tells it.

use std::fmt::Write as _;
use std::io::Write;

use serde_json::Value;

use super::{Done, show};
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventKind, Heard, kinds};
use crate::api::{self, Error, Scope, Spec};
use crate::hotkeys;

/// Print every command, or those whose id or title has `search`, by id:
/// as a table, JSON, or the command reference.
pub(super) fn actions(
    out: &mut dyn Write,
    json: bool,
    markdown: bool,
    search: Option<&str>,
) -> Done {
    if markdown {
        return Ok(write!(out, "{}", reference())?);
    }
    let found = found(search);
    if let Some(search) = search.filter(|_| found.is_empty()) {
        return Err(Error::not_found("command", search).into());
    }
    if json {
        let described: Value = found.into_iter().map(Spec::describe).collect();
        return Ok(show::pretty(out, &described)?);
    }
    let rows: Vec<Vec<String>> = found
        .into_iter()
        .map(|spec| vec![spec.id.to_owned(), spec.title.to_owned(), keys(spec)])
        .collect();
    Ok(show::table(out, &["ID", "TITLE", "KEYS"], &rows)?)
}

/// The commands whose id or title has `search`, in any case (ids are
/// lowercase); every one without it. By id, alphabetically.
fn found(search: Option<&str>) -> Vec<&'static Spec> {
    let search = search.map(str::to_lowercase);
    api::by_id()
        .into_iter()
        .filter(|spec| {
            search.as_deref().is_none_or(|search| {
                spec.id.contains(search) || spec.title.to_lowercase().contains(search)
            })
        })
        .collect()
}

/// The keys a command starts out with, as menus show them.
fn keys(spec: &Spec) -> String {
    let shown: Vec<String> = spec.keys.iter().map(|key| hotkeys::display(key)).collect();
    shown.join(", ")
}

/// The command reference, in Markdown: each namespace's commands, with
/// what they act on, their keys, arguments, what they hand back and the
/// events they tell; then each namespace's events, with who hears them,
/// their payload, and what tells them.
pub fn reference() -> String {
    let mut text = String::from(
        "# Commands\n\n\
         Every command Plays has, by namespace, as `plays actions --markdown` writes \
         them. A terminal runs one with `plays do ID [KEY=VALUE…]`, naming the window \
         or block it acts on with `-w` and `-b`; keys run those that have them, and \
         Settings → Hotkeys changes those. The keys here are macOS's: elsewhere Ctrl \
         stands for ⌘, and Alt for ⌥. What each tells is in [Events](#events).\n",
    );
    let mut namespace = "";
    for spec in commands() {
        if spec.namespace() != namespace {
            namespace = spec.namespace();
            writeln!(text, "\n## {namespace}").ok();
        }
        command(&mut text, spec);
    }
    text.push_str(
        "\n# Events\n\n\
         What changes in Plays, its windows and their blocks, by namespace: `plays events` \
         prints each as it comes, and `plays wait` waits for one. Each is a JSON object, \
         `{type, payload}`. The payload of one about a window starts with `window_id` and \
         `window_title`, as its title bar shows it; of one about a block, with `block_id` \
         and `block_title` too, as its header shows it. A listener hears the app's, one \
         window's, or all of them. Readers ignore events and fields they don't know.\n\n\
         A cue is a block reaching a time or a frame: `block.<block>.position@120.5s`, or \
         `…@345f`, the block by id, label or number. `plays wait` waits for one and \
         `plays events --cue` hears them: each once, the cue its type, with \
         `block.playback.position`'s payload.\n",
    );
    let mut namespace = "";
    for (in_namespace, kind) in kinds::all() {
        if in_namespace != namespace {
            namespace = in_namespace;
            writeln!(text, "\n## {namespace}").ok();
        }
        event(&mut text, kind);
    }
    text
}

/// The commands the reference has: those for working on Plays itself are
/// in debug builds only.
fn commands() -> impl Iterator<Item = &'static Spec> {
    api::all().filter(|spec| !spec.developer)
}

/// One event, in the reference.
fn event(text: &mut String, kind: &EventKind) {
    writeln!(text, "\n### {}\n\n{}\n", kind.name, kind.description).ok();
    let heard = match kind.heard {
        Heard::App => "those listening to the app's events",
        Heard::AppAndWindow => "those listening to the app's events, and to its window's",
        Heard::Window | Heard::Block => "those listening to its window's events",
        Heard::Everyone => "every listener",
    };
    writeln!(text, "- Heard by {heard}.").ok();
    let ids = kind.heard.ids();
    if ids.is_empty() && kind.fields.is_empty() {
        text.push_str("- Payload: none.\n");
    } else {
        text.push_str("- Payload:\n");
    }
    if !ids.is_empty() {
        let names: Vec<String> = ids.iter().map(|id| format!("`{id}`")).collect();
        let whose = if kind.heard == Heard::Block {
            "its window's and its block's"
        } else {
            "its window's"
        };
        writeln!(text, "  - {}: {whose}.", names.join(", ")).ok();
    }
    for field in kind.fields {
        let sometimes = if field.optional {
            ", only sometimes"
        } else {
            ""
        };
        writeln!(
            text,
            "  - `{}`{sometimes}: {}",
            field.name, field.description
        )
        .ok();
    }
    let mut follows: Vec<String> = commands()
        .filter(|spec| spec.tells.contains(&kind.name))
        .map(|spec| format!("`{}`", spec.id))
        .collect();
    follows.extend(kind.causes.iter().map(|&cause| cause.to_owned()));
    if let Some(last) = follows.len().checked_sub(1).filter(|&last| last > 0) {
        follows[last] = format!("or {}", follows[last]);
    }
    writeln!(text, "- Follows: {}.", follows.join(", ")).ok();
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
    if !spec.tells.is_empty() {
        let tells: Vec<String> = spec.tells.iter().map(|kind| format!("`{kind}`")).collect();
        writeln!(text, "- Tells: {}.", tells.join(", ")).ok();
    }
}

fn acts_on(scope: Scope) -> &'static str {
    match scope {
        Scope::App => "the app",
        Scope::AnyWindow => "a window, Settings and About too",
        Scope::Window => "a player window",
        Scope::Blocks => "every block in a window",
        Scope::EveryBlock => "every block in every window",
        Scope::Block => "a block",
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
        Kind::Toggle => "true or false".to_owned(),
        Kind::Choice(choices) => format!("one of {}", choices().join(", ")),
        Kind::Choices(choices) => format!("a list of any of {}, none too", choices().join(", ")),
        Kind::Any => "any value".to_owned(),
    };
    if arg.required {
        kind
    } else {
        format!("{kind}, optional")
    }
}
