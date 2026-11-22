//! The scripts that complete a clap command line in bash, zsh, fish and
//! Nushell: clap's own (`clap_complete`, `clap_complete_nushell`), with
//! what they leave out filled in. In fish, clap's completes options alone,
//! so lines are added offering the names a word takes (the possible values
//! of a positional argument), each with what it is, until one is given.
//! In Nushell, clap's completers list the names alone, so each lists
//! records instead, a name with what it is.

use std::fmt::Write as _;

use clap::Command;
use clap::builder::PossibleValue;
use clap_complete_nushell::Nushell;

/// A shell a script completes the command line in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Nushell,
}

/// The script that has `shell` complete `command`, run as `bin`.
pub fn script(shell: Shell, command: &mut Command, bin: &str) -> String {
    // Whole before it's handed out: clap panics if it can't write, as to a
    // pipe closed early.
    let mut script = Vec::new();
    let mut generate = |generator| clap_complete::generate(generator, command, bin, &mut script);
    match shell {
        Shell::Bash => generate(clap_complete::Shell::Bash),
        Shell::Zsh => generate(clap_complete::Shell::Zsh),
        Shell::Fish => generate(clap_complete::Shell::Fish),
        Shell::Nushell => clap_complete::generate(Nushell, command, bin, &mut script),
    }
    let script = String::from_utf8_lossy(&script).into_owned();
    match shell {
        Shell::Bash | Shell::Zsh => script,
        Shell::Fish => script + &fish_words(command, bin),
        Shell::Nushell => nu_described(&script, command),
    }
}

/// What clap's script for fish leaves out, as it completes options
/// alone: the words with names to offer, each offered until one of its
/// names is given.
fn fish_words(command: &Command, bin: &str) -> String {
    let mut lines = String::from("\n# The words that aren't options, and the names each takes.\n");
    for sub in command.get_subcommands() {
        fish_offer_within(&mut lines, bin, &[sub.get_name()], sub);
    }
    lines
}

/// The lines offering the names the words of `command`, and of each
/// subcommand within it, take; `path` being the subcommands that name it.
fn fish_offer_within(lines: &mut String, bin: &str, path: &[&str], command: &Command) {
    fish_offer(lines, bin, path, command);
    for nested in command.get_subcommands() {
        let within = [path, &[nested.get_name()]].concat();
        fish_offer_within(lines, bin, &within, nested);
    }
}

/// The lines offering the names `command`'s words take, `path` being the
/// subcommands that name it.
fn fish_offer(lines: &mut String, bin: &str, path: &[&str], command: &Command) {
    let [first, within @ ..] = path else {
        return;
    };
    for arg in command.get_positionals() {
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
        let offered: Vec<String> = values
            .iter()
            .map(|value| {
                let help = value
                    .get_help()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                format!("{}\\t'{}'", fish_text(value.get_name()), fish_text(&help))
            })
            .collect();
        writeln!(
            lines,
            "complete -c {bin} -n \"{when}\" -f -a \"{}\"",
            offered.join("\n")
        )
        .ok();
    }
}

/// `text` to be read as it is from between single quotes within double
/// ones, as fish reads what `complete -a` offers: first as the double
/// quotes are read, then the single ones.
fn fish_text(text: &str) -> String {
    text.replace('\\', "\\\\\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

/// `script`, clap's for Nushell, with what it leaves out: what each name
/// it offers is beside it. Its completers list the names alone; each lists
/// records instead.
fn nu_described(script: &str, command: &Command) -> String {
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
                script.replace_range(at..at + end, &nu_records(arg.get_possible_values()));
            }
        }
    }
    script
}

/// `values` as a completer's list of records, each with what it is where
/// it says.
fn nu_records(values: Vec<PossibleValue>) -> String {
    let mut records = String::from("\n    [\n");
    for value in values {
        write!(records, "      {{ value: \"{}\"", nu_text(value.get_name())).ok();
        if let Some(help) = value.get_help() {
            write!(records, ", description: \"{}\"", nu_text(&help.to_string())).ok();
        }
        records.push_str(" }\n");
    }
    records.push_str("    ]");
    records
}

/// `text` to be read as it is from between double quotes, as Nushell reads
/// a string: backslashes and quotes escaped. Nothing in one is run.
fn nu_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
#[path = "tests/completions.rs"]
mod tests;
