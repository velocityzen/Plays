//! `plays completions SHELL`: the script that has bash, zsh or fish
//! complete the command line, its subcommands and options, the ids of
//! Plays' commands for `do` and `api`, setting keys, and files to open.
//! It's written from what clap knows of the command line and the registry
//! this program has, so Plays needn't be running; one built with other
//! commands writes them.

use std::fmt::Write as _;
use std::io::{self, Write};
use std::sync::Arc;

use clap::builder::{PossibleValue, PossibleValuesParser};
use clap::{Command, CommandFactory};

use super::args::{Cli, Shell};
use crate::api;
use crate::cast::{self, Pairings};
use crate::files::Folder;
use crate::settings::fields;

/// Print the script that completes `plays` in `shell`.
pub fn completions(out: &mut dyn Write, shell: Shell) -> io::Result<()> {
    // Whole before it's printed: clap panics if it can't write, as to a
    // pipe closed early.
    let mut script = Vec::new();
    let mut command = command();
    clap_complete::generate(generator(shell), &mut command, "plays", &mut script);
    if matches!(shell, Shell::Fish) {
        script.extend_from_slice(fish_words(&command).as_bytes());
    }
    out.write_all(&script)
}

fn generator(shell: Shell) -> clap_complete::Shell {
    match shell {
        Shell::Bash => clap_complete::Shell::Bash,
        Shell::Zsh => clap_complete::Shell::Zsh,
        Shell::Fish => clap_complete::Shell::Fish,
    }
}

/// The command line as clap reads it, with the names each word can be:
/// the commands' ids, each with its title, and the settings' keys.
fn command() -> Command {
    let commands = || named(api::all().map(|spec| (spec.id.to_owned(), spec.title.to_owned())));
    let keys = named(
        fields::all(&protocols())
            .into_iter()
            .map(|field| (field.key.into_owned(), field.title.into_owned())),
    );
    Cli::command()
        .mut_subcommand("do", |run| offer(run, "command", commands()))
        .mut_subcommand("api", |api| {
            api.mut_subcommand("describe", |describe| {
                offer(describe, "command", commands())
            })
            .mut_subcommand("call", |call| offer(call, "command", commands()))
        })
        .mut_subcommand("settings", |settings| {
            settings
                .mut_subcommand("get", |get| offer(get, "key", keys.clone()))
                .mut_subcommand("set", |set| offer(set, "key", keys))
        })
}

/// `names`, each with what it is.
fn named(names: impl Iterator<Item = (String, String)>) -> Vec<PossibleValue> {
    names
        .map(|(name, help)| PossibleValue::new(name).help(help))
        .collect()
}

/// `command` with `values` for its argument `name`. Changed where it is,
/// as `mut_arg` would put it last, after those it comes before.
fn offer(command: Command, name: &str, values: Vec<PossibleValue>) -> Command {
    command.mut_args(|arg| {
        if arg.get_id() == name {
            arg.value_parser(PossibleValuesParser::new(values.clone()))
        } else {
            arg
        }
    })
}

/// What clap's script for fish leaves out, as it completes options
/// alone: the words with names to offer, a command's id or a setting's
/// key, each offered until one of its names is given.
fn fish_words(command: &Command) -> String {
    let mut lines =
        String::from("\n# The words that aren't options: commands' ids and settings' keys.\n");
    for sub in command.get_subcommands() {
        fish_offer(&mut lines, &[sub.get_name()], sub);
        for nested in sub.get_subcommands() {
            fish_offer(&mut lines, &[sub.get_name(), nested.get_name()], nested);
        }
    }
    lines
}

/// The lines offering the names `command`'s words take, `path` being the
/// subcommands that name it.
fn fish_offer(lines: &mut String, path: &[&str], command: &Command) {
    let [first, within @ ..] = path else {
        return;
    };
    for arg in command.get_positionals() {
        let values = arg.get_possible_values();
        if values.is_empty() {
            continue;
        }
        let mut when = format!("__fish_plays_using_subcommand {first}");
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
            "complete -c plays -n \"{when}\" -f -a \"{}\"",
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

/// The kinds of device Plays casts to, each of which has a setting for
/// whether it looks for them. None is started to say.
fn protocols() -> Vec<&'static str> {
    cast::protocols(Arc::new(Pairings::load(&Folder::default())))
        .iter()
        .map(|protocol| protocol.kind)
        .collect()
}

#[cfg(test)]
mod tests;
