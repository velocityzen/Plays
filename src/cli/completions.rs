//! `plays completions SHELL`: the script that has bash, zsh, fish or
//! Nushell complete the command line, its subcommands and options, the ids of
//! Plays' commands for `do` and `api`, and with their namespaces for
//! `actions`, setting keys, files to open, and Lua scripts for `do`.
//! It's written from what clap knows of the command line and the registry
//! this program has, so Plays needn't be running; one built with other
//! commands writes them. What clap's scripts leave out, crate
//! `completions` fills in; what each word can be is Plays' own.

use std::io::{self, Write};
use std::sync::Arc;

use ::completions::Files;
use clap::builder::{PossibleValue, PossibleValuesParser};
use clap::{Command, CommandFactory};

use super::args::{Cli, Shell};
use crate::api;
use crate::cast::{self, Pairings};
use crate::files::Folder;
use crate::settings::fields;

/// Print the script that completes `plays` in `shell`.
pub fn completions(out: &mut dyn Write, shell: Shell) -> io::Result<()> {
    let shell = match shell {
        Shell::Bash => ::completions::Shell::Bash,
        Shell::Zsh => ::completions::Shell::Zsh,
        Shell::Fish => ::completions::Shell::Fish,
        Shell::Nushell => ::completions::Shell::Nushell,
    };
    let scripts = Files {
        path: &["do"],
        suffix: ".lua",
    };
    out.write_all(::completions::script(shell, &mut command(), "plays", &[scripts]).as_bytes())
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
    let namespaces = api::commands::NAMESPACES
        .iter()
        .map(|&(id, title)| (id.to_owned(), title.to_owned()));
    let searches = named(namespaces).into_iter().chain(commands()).collect();
    Cli::command()
        .mut_subcommand("do", |run| offer(run, "command", commands()))
        .mut_subcommand("actions", |actions| offer(actions, "search", searches))
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

/// The kinds of device Plays casts to, each of which has a setting for
/// whether it looks for them. None is started to say.
fn protocols() -> Vec<&'static str> {
    cast::protocols(Arc::new(Pairings::load(&Folder::default())))
        .iter()
        .map(|protocol| protocol.kind)
        .collect()
}

#[cfg(test)]
#[path = "tests/completions.rs"]
mod tests;
