//! Nushell: clap's completers list the names alone, so each lists records
//! instead, a name with what it is; and a word that takes files lists them
//! too.

use std::fmt::Write as _;

use clap::Command;
use clap::builder::PossibleValue;

use crate::{Files, first_word};

/// `script`, clap's for Nushell, with what it leaves out: what each name
/// it offers is beside it. Its completers list the names alone; each lists
/// records instead.
pub(crate) fn described(script: &str, command: &Command) -> String {
    let mut script = script.to_owned();
    let mut commands = vec![command];
    while let Some(command) = commands.pop() {
        commands.extend(command.get_subcommands());
        let bin = command.get_bin_name().unwrap_or(command.get_name());
        for arg in command.get_arguments() {
            let head = format!("def \"nu-complete {bin} {}\" [] {{", arg.get_id());
            let Some(at) = script.find(&head).map(|at| at + head.len()) else {
                continue;
            };
            if let Some(end) = script[at..].find("\n  }") {
                script.replace_range(at..at + end, &records(arg.get_possible_values()));
            }
        }
    }
    script
}

/// Have Nushell offer `files` for their word, besides its names, in
/// `script`: its completer lists them after its names, from the folder of
/// the word typed, which it's given.
pub(crate) fn with_files(script: &mut String, command: &Command, files: &Files) {
    let Some((command, word)) = first_word(command, files.path) else {
        return;
    };
    let bin = command.get_bin_name().unwrap_or(command.get_name());
    let head = format!("def \"nu-complete {bin} {word}\" [] {{");
    let Some(start) = script.find(&head) else {
        return;
    };
    let Some(end) = script[start..].find("\n  }").map(|end| start + end) else {
        return;
    };
    let records = script[start + head.len()..end].to_owned();
    let head = head.replace(" [] {", " [context: string] {");
    let suffix = text(files.suffix);
    let completer = format!(
        "{head}\n    let typed = $context | split row \" \" | last\n    \
         let folder = $typed | str replace --regex '[^/]*$' ''\n    \
         let listed = try {{ ls (if $folder == \"\" {{ \".\" }} else {{ $folder }}) }} \
         catch {{ [] }}{records} | append (\n      $listed\n      \
         | where {{|file| $file.type == dir or ($file.name | str ends-with \"{suffix}\") }}\n      \
         | each {{|file| if $file.type == dir {{ {{ value: $\"($file.name)/\" }} }} \
         else {{ {{ value: $file.name }} }} }}\n    )"
    );
    script.replace_range(start..end, &completer);
}

/// `values` as a completer's list of records, each with what it is where
/// it says.
fn records(values: Vec<PossibleValue>) -> String {
    let mut records = String::from("\n    [\n");
    for value in values {
        write!(records, "      {{ value: \"{}\"", text(value.get_name())).ok();
        if let Some(help) = value.get_help() {
            write!(records, ", description: \"{}\"", text(&help.to_string())).ok();
        }
        records.push_str(" }\n");
    }
    records.push_str("    ]");
    records
}

/// `text` to be read as it is from between double quotes, as Nushell reads
/// a string: backslashes and quotes escaped. Nothing in one is run.
fn text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
#[path = "tests/nushell.rs"]
mod tests;
