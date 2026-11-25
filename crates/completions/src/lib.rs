//! The scripts that complete a clap command line in bash, zsh, fish and
//! Nushell: clap's own (`clap_complete`, `clap_complete_nushell`), with
//! what they leave out filled in, each shell's in a module of its own. A
//! word may take files too, besides its names (`Files`): those ending in a
//! suffix, and folders to find them in.

#![forbid(unsafe_code)]

mod bash;
mod fish;
mod nushell;
mod zsh;

use clap::Command;
use clap_complete_nushell::Nushell;

/// A shell a script completes the command line in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Nushell,
}

/// A word that takes files besides its names: the first of the
/// subcommand's at `path`, offering folders and the files whose names end
/// in `suffix`.
#[derive(Clone, Copy, Debug)]
pub struct Files<'a> {
    pub path: &'a [&'a str],
    pub suffix: &'a str,
}

/// The script that has `shell` complete `command`, run as `bin`, `files`
/// naming the words that take files too.
pub fn script(shell: Shell, command: &mut Command, bin: &str, files: &[Files]) -> String {
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
    let mut script = String::from_utf8_lossy(&script).into_owned();
    match shell {
        Shell::Bash => files
            .iter()
            .for_each(|files| bash::with_files(&mut script, files)),
        Shell::Zsh => files
            .iter()
            .for_each(|files| zsh::with_files(&mut script, command, bin, files)),
        Shell::Fish => script += &fish::words(command, bin, files),
        Shell::Nushell => {
            script = nushell::described(&script, command);
            files
                .iter()
                .for_each(|files| nushell::with_files(&mut script, command, files));
        }
    }
    script
}

/// The subcommand at `path` within `command`, and its first word.
pub(crate) fn first_word<'a>(
    command: &'a Command,
    path: &[&str],
) -> Option<(&'a Command, &'a str)> {
    let mut command = command;
    for name in path {
        command = command.find_subcommand(name)?;
    }
    Some((command, command.get_positionals().next()?.get_id().as_str()))
}

#[cfg(test)]
#[path = "tests/completions.rs"]
mod tests;
