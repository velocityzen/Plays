//! `plays actions`: every command, or those a search finds, from the
//! registry this program has, so Plays needn't be running. As a table of
//! ids, titles and keys; as JSON, each described; or, with `--markdown`,
//! the reference kept in `docs/commands.md`: every command and every event
//! Plays tells, by function.

use std::collections::HashMap;
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

/// The functions the reference groups commands and events by, in its
/// order: each one's title, and the word its parts' headings call it by.
const FUNCTIONS: [(&str, &str); 13] = [
    ("Playback", "Playback"),
    ("Audio", "Audio"),
    ("Subtitles", "Subtitles"),
    ("Video", "Video"),
    ("Interface", "Interface"),
    ("Blocks", "Block"),
    ("Windows", "Window"),
    ("Casting", "Casting"),
    ("Audio output", "Audio output"),
    ("Displays", "Display"),
    ("Settings", "Settings"),
    ("App", "App"),
    ("Command line", "Command-line"),
];

/// Where a function's commands act, as their ids start, from the
/// narrowest, and what its parts' headings say of each when it has more
/// than one.
const SCOPES: [(&str, &str); 3] = [
    ("block", "in a block"),
    ("window", "in a window"),
    ("app", "in every window"),
];

/// The function a command's or an event's id is of, as `FUNCTIONS` titles
/// it: as the menus have them.
fn function(id: &str) -> &'static str {
    let parts: Vec<&str> = id.split('.').collect();
    // A feature's, as `playback` is in `window.playback.play`; not a
    // command's own name, as `close` is in `block.close`.
    let feature = if parts.len() > 2 { parts[1] } else { "" };
    match (parts[0], feature) {
        (_, "playback") => "Playback",
        (_, "audio") => "Audio",
        (_, "subtitles") => "Subtitles",
        (_, "video" | "picture_in_picture" | "console") => "Video",
        (_, "ui" | "controls" | "top_bar" | "track_switcher" | "notes") => "Interface",
        ("block", _) => "Blocks",
        ("window", _) => "Windows",
        ("cast", _) => "Casting",
        ("output", _) => "Audio output",
        ("display", _) => "Displays",
        ("app", "settings") => "Settings",
        ("cli", _) => "Command line",
        _ => "App",
    }
}

/// The anchors GitHub gives headings, as they come: lowercase, without
/// what isn't a letter, a digit, a space, `-` or `_`, a space as `-`; one
/// met before ends `-1`, `-2`…
#[derive(Default)]
struct Anchors {
    seen: HashMap<String, usize>,
    /// Each command's, by id, and each event's, by type.
    of: HashMap<&'static str, String>,
}

impl Anchors {
    /// The next heading's.
    fn next(&mut self, heading: &str) -> String {
        let slug: String = heading
            .to_lowercase()
            .chars()
            .filter_map(|c| match c {
                ' ' => Some('-'),
                c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
                _ => None,
            })
            .collect();
        let seen = self.seen.entry(slug.clone()).or_insert(0);
        let anchor = if *seen == 0 {
            slug
        } else {
            format!("{slug}-{seen}")
        };
        *seen += 1;
        anchor
    }

    /// A link to the command `id`, or the event `id`, by its id.
    fn link(&self, id: &str) -> String {
        format!("[`{id}`](#{})", self.of[id])
    }
}

/// A function's commands that act in one place, or its events.
struct Part {
    heading: String,
    /// What the contents call it: in a block, commands or events.
    label: &'static str,
    anchor: String,
    commands: Vec<&'static Spec>,
    events: Vec<&'static EventKind>,
}

struct Function {
    title: &'static str,
    anchor: String,
    parts: Vec<Part>,
}

/// The reference's functions, with their parts, each command and event's
/// anchor noted as its heading comes.
fn functions(anchors: &mut Anchors) -> Vec<Function> {
    let mut functions = Vec::new();
    for (title, word) in FUNCTIONS {
        let commands: Vec<&Spec> = commands()
            .filter(|spec| function(spec.id) == title)
            .collect();
        let events: Vec<&EventKind> = kinds::all()
            .map(|(_, kind)| kind)
            .filter(|kind| function(kind.name) == title)
            .collect();
        let scope = |spec: &Spec| spec.id.split('.').next().unwrap_or(spec.id);
        let mut scopes: Vec<&str> = Vec::new();
        for spec in &commands {
            if !scopes.contains(&scope(spec)) {
                scopes.push(scope(spec));
            }
        }
        scopes.sort_by_key(|&at| SCOPES.iter().position(|&(scope, _)| scope == at));
        let anchor = anchors.next(title);
        let mut parts = Vec::new();
        for at in &scopes {
            let (heading, label) = match SCOPES.iter().find(|&&(scope, _)| scope == *at) {
                Some(&(_, place)) if scopes.len() > 1 => (format!("{word} {place}"), place),
                _ => (format!("{word} commands"), "commands"),
            };
            let anchor = anchors.next(&heading);
            let commands: Vec<&Spec> = commands
                .iter()
                .copied()
                .filter(|spec| scope(spec) == *at)
                .collect();
            for spec in &commands {
                let anchor = anchors.next(spec.title);
                anchors.of.insert(spec.id, anchor);
            }
            parts.push(Part {
                heading,
                label,
                anchor,
                commands,
                events: Vec::new(),
            });
        }
        if !events.is_empty() {
            let heading = format!("{word} events");
            let anchor = anchors.next(&heading);
            for kind in &events {
                let anchor = anchors.next(kind.name);
                anchors.of.insert(kind.name, anchor);
            }
            parts.push(Part {
                heading,
                label: "events",
                anchor,
                commands: Vec::new(),
                events,
            });
        }
        if !parts.is_empty() {
            functions.push(Function {
                title,
                anchor,
                parts,
            });
        }
    }
    functions
}

/// The command reference, in Markdown, by function: contents first, then
/// how commands run and events are heard, then each function's commands,
/// for a block, a window or every window, with what they act on, their
/// keys, arguments, what they return and the events they cause; and its
/// events, with who hears them, their payload, and what causes them.
pub fn reference() -> String {
    let mut anchors = Anchors::default();
    for heading in ["Commands and events", "Contents", "Commands", "Events"] {
        anchors.next(heading);
    }
    let functions = functions(&mut anchors);
    let mut text = String::from(
        "# Commands and events\n\n\
         Every command Plays has and every event it tells, by what they're about, as \
         `plays actions --markdown` writes them.\n\n\
         ## Contents\n\n\
         - [Commands](#commands) and [Events](#events): running the one, hearing the other\n",
    );
    for function in &functions {
        let parts: Vec<String> = function
            .parts
            .iter()
            .map(|part| format!("[{}](#{})", part.label, part.anchor))
            .collect();
        writeln!(
            text,
            "- [{}](#{}): {}",
            function.title,
            function.anchor,
            parts.join(", ")
        )
        .ok();
    }
    text.push_str(
        "\n## Commands\n\n\
         A terminal runs one with `plays do ID [KEY=VALUE…]`, naming the window or block it \
         acts on with `-w` and `-b`; keys run those that have them, and Settings → Hotkeys \
         changes those. The keys here are macOS's: elsewhere Ctrl stands for ⌘, and Alt for \
         ⌥. A command for a window, or for every window, does to each block in it what the \
         block's does, its title ending in Window, or in Every Window. Each lists its \
         arguments, given as `KEY=VALUE`; what it returns, as `plays api call` prints it; and \
         the events it causes.\n\n\
         ## Events\n\n\
         What changes in Plays, its windows and their blocks: `plays events` prints each as \
         it comes, and `plays wait` waits for one. Each is a JSON object, `{type, payload}`. \
         The payload of one about a window starts with `window_id` and `window_title`, as its \
         title bar shows it; of one about a block, with `block_id` and `block_title` too, as \
         its header shows it. A listener hears the app's, one window's, or all of them. \
         Readers ignore events and fields they don't know. An event is heard, not run: it \
         has a payload, not arguments or a return value. Each lists who hears it, its \
         payload, and what causes it.\n\n\
         A cue is a block reaching a time or a frame: `block.<block>.position@120.5s`, or \
         `…@345f`, the block by id, label or number. `plays wait` waits for one and \
         `plays events --cue` hears them: each once, the cue its type, with \
         `block.playback.position`'s payload.\n",
    );
    for function in &functions {
        writeln!(text, "\n## {}", function.title).ok();
        for part in &function.parts {
            writeln!(text, "\n### {}", part.heading).ok();
            for spec in &part.commands {
                command(&mut text, spec, &anchors);
            }
            for kind in &part.events {
                event(&mut text, kind, &anchors);
            }
        }
    }
    text
}

/// The commands the reference has: those for working on Plays itself are
/// in debug builds only.
fn commands() -> impl Iterator<Item = &'static Spec> {
    api::all().filter(|spec| !spec.developer)
}

/// One event, in the reference.
fn event(text: &mut String, kind: &EventKind, anchors: &Anchors) {
    writeln!(text, "\n#### {}\n\n{}\n", kind.name, kind.description).ok();
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
    let mut causes: Vec<String> = commands()
        .filter(|spec| spec.tells.contains(&kind.name))
        .map(|spec| anchors.link(spec.id))
        .collect();
    causes.extend(kind.causes.iter().map(|&cause| cause.to_owned()));
    if let Some(last) = causes.len().checked_sub(1).filter(|&last| last > 0) {
        causes[last] = format!("or {}", causes[last]);
    }
    writeln!(text, "- Caused by: {}.", causes.join(", ")).ok();
}

/// One command, in the reference.
fn command(text: &mut String, spec: &Spec, anchors: &Anchors) {
    writeln!(
        text,
        "\n#### {}\n\n`{}`: {}\n",
        spec.title, spec.id, spec.description
    )
    .ok();
    writeln!(text, "- Acts on {}.", acts_on(spec.scope)).ok();
    if !spec.keys.is_empty() {
        writeln!(text, "- Keys: {}", keys(spec)).ok();
    }
    if spec.args.is_empty() {
        text.push_str("- Arguments: none.\n");
    } else {
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
    match spec.returns {
        Some(returns) => writeln!(text, "- Returns: {returns}").ok(),
        None => writeln!(text, "- Returns: nothing.").ok(),
    };
    if spec.tells.is_empty() {
        text.push_str("- Events: none.\n");
    } else {
        let events: Vec<String> = spec.tells.iter().map(|kind| anchors.link(kind)).collect();
        writeln!(text, "- Events: {}.", events.join(", ")).ok();
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
