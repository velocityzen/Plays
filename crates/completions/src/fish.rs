//! fish: clap's script completes options alone, so lines are added
//! offering the names a word takes (the possible values of a positional
//! argument), each with what it is, until one is given; and the files a
//! word takes too.

use std::fmt::Write as _;

use clap::Command;
use clap::builder::PossibleValue;

use crate::Files;

/// What clap's script for fish leaves out, as it completes options
/// alone: the words with names to offer, each offered until one of its
/// names is given, or a file of those `files` offer.
pub(crate) fn words(command: &Command, bin: &str, files: &[Files]) -> String {
    let mut lines = String::from("\n# The words that aren't options, and the names each takes.\n");
    if !files.is_empty() {
        // The folders and the files ending in the suffix given, of those
        // in the folder of the word typed.
        writeln!(
            lines,
            "function {} -a suffix\n    \
             for file in (commandline -ct)*\n        \
             if test -d $file\n            echo $file/\n        \
             else if string match -q -- \"*$suffix\" $file\n            echo $file\n        \
             end\n    end\nend",
            files_ending(bin)
        )
        .ok();
    }
    for sub in command.get_subcommands() {
        offer_within(&mut lines, bin, &[sub.get_name()], sub, files);
    }
    lines
}

/// The name of the function listing files for a word that takes them.
fn files_ending(bin: &str) -> String {
    format!("__fish_{}_files_ending", bin.replace('-', "_"))
}

/// The lines offering the names the words of `command`, and of each
/// subcommand within it, take; `path` being the subcommands that name it.
fn offer_within(lines: &mut String, bin: &str, path: &[&str], command: &Command, files: &[Files]) {
    offer(lines, bin, path, command, files);
    for nested in command.get_subcommands() {
        let within = [path, &[nested.get_name()]].concat();
        offer_within(lines, bin, &within, nested, files);
    }
}

/// The lines offering the names `command`'s words take, and the files its
/// first takes if `files` has it take some, `path` being the subcommands
/// that name it.
fn offer(lines: &mut String, bin: &str, path: &[&str], command: &Command, files: &[Files]) {
    let [first, within @ ..] = path else {
        return;
    };
    let suffix = files
        .iter()
        .find(|files| files.path == path)
        .map(|files| files.suffix);
    for (at, arg) in command.get_positionals().enumerate() {
        let values = arg.get_possible_values();
        if values.is_empty() {
            continue;
        }
        // As clap names it in its script.
        let using = format!("__fish_{}_using_subcommand", bin.replace('-', "_"));
        let mut when = format!("{using} {first}");
        for word in within {
            write!(when, "; and __fish_seen_subcommand_from {word}").ok();
        }
        let names: Vec<&str> = values.iter().map(PossibleValue::get_name).collect();
        write!(
            when,
            "; and not __fish_seen_subcommand_from {}",
            names.join(" ")
        )
        .ok();
        // Nor once a file it takes is given.
        let suffix = suffix.filter(|_| at == 0).map(text);
        if let Some(suffix) = &suffix {
            write!(
                when,
                "; and not string match -q -- '*{suffix}' (commandline -opc)"
            )
            .ok();
        }
        let offered: Vec<String> = values
            .iter()
            .map(|value| {
                let help = value
                    .get_help()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                format!("{}\\t'{}'", text(value.get_name()), text(&help))
            })
            .collect();
        writeln!(
            lines,
            "complete -c {bin} -n \"{when}\" -f -a \"{}\"",
            offered.join("\n")
        )
        .ok();
        if let Some(suffix) = suffix {
            let listed = files_ending(bin);
            writeln!(
                lines,
                "complete -c {bin} -n \"{when}\" -f -a \"({listed} '{suffix}')\""
            )
            .ok();
        }
    }
}

/// `text` to be read as it is from between single quotes within double
/// ones, as fish reads what `complete -a` offers: first as the double
/// quotes are read, then the single ones.
fn text(text: &str) -> String {
    text.replace('\\', "\\\\\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

#[cfg(test)]
#[path = "tests/fish.rs"]
mod tests;
