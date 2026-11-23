//! A small command line of the tests' own, and what each shell's tests
//! share: a folder with its script in it, and asking a shell, where it's
//! installed, as a terminal or an editor would.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Output};

use clap::builder::{PossibleValue, PossibleValuesParser};
use clap::{Arg, ValueHint};

use super::*;

/// What its names say, as no shell should read it: quotes, a variable,
/// a command to run and backslashes.
pub(crate) const AWKWARD: &str = r#"it's "quoted", $HOME, (not run), a back\slash and two \\"#;

/// The command line the tests complete, run as `my-probe`: `play` takes a
/// word with names, given always, `config get` a key, nested a level down,
/// and `open` files.
pub(crate) fn probe() -> Command {
    let words = PossibleValuesParser::new([
        PossibleValue::new("one").help("The first"),
        PossibleValue::new("awkward").help(AWKWARD),
        PossibleValue::new("bare"),
    ]);
    let keys = PossibleValuesParser::new([
        PossibleValue::new("size.small").help("Small"),
        PossibleValue::new("size.large").help("Large"),
    ]);
    Command::new("my-probe")
        .subcommand(Command::new("play").arg(Arg::new("word").required(true).value_parser(words)))
        .subcommand(
            Command::new("config")
                .subcommand(Command::new("get").arg(Arg::new("key").value_parser(keys))),
        )
        .subcommand(
            Command::new("open").arg(
                Arg::new("paths")
                    .num_args(0..)
                    .value_hint(ValueHint::FilePath),
            ),
        )
}

/// A folder of a test's own, with the script for a shell in it: removed
/// once the test is done with it, whether it passed or not.
pub(crate) struct Scratch {
    pub(crate) folder: PathBuf,
    pub(crate) script: PathBuf,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

/// The words of the tests' command line that take files: `play`'s, Lua
/// scripts besides its names.
pub(crate) const SCRIPTS: Files = Files {
    path: &["play"],
    suffix: ".lua",
};

/// A folder of the test's own, named for it, with the script for `shell`
/// in it as `file`, `files` naming the words that take files.
pub(crate) fn with_script(name: &str, shell: Shell, file: &str, files: &[Files]) -> Scratch {
    let folder = std::env::temp_dir()
        .join("completions-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(file);
    fs::write(&path, script(shell, &mut probe(), "my-probe", files)).unwrap();
    Scratch {
        folder,
        script: path,
    }
}

#[test]
fn a_tests_folder_goes_once_it_is_done_with() {
    let scratch = with_script("gone", Shell::Bash, "my-probe.bash", &[]);
    let folder = scratch.folder.clone();
    assert!(scratch.script.is_file());
    drop(scratch);
    assert!(!folder.exists());
}

/// Each line `output` printed, having printed nothing on stderr.
pub(crate) fn printed(output: &Output) -> Vec<String> {
    assert!(
        output.status.success() && output.stderr.is_empty(),
        "{output:?}"
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// What `shell` prints for `args`, in `folder`, or nothing if there's no
/// such shell to ask.
pub(crate) fn ask(shell: &str, folder: &Path, args: &[&str]) -> Option<Vec<String>> {
    match Process::new(shell).args(args).current_dir(folder).output() {
        Ok(output) => Some(printed(&output)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            eprintln!("no {shell} to ask: what it offers goes unchecked");
            None
        }
        Err(err) => panic!("{err}"),
    }
}

#[test]
fn bash_and_zsh_have_clap_scripts_as_they_are() {
    for (shell, clap) in [
        (Shell::Bash, clap_complete::Shell::Bash),
        (Shell::Zsh, clap_complete::Shell::Zsh),
    ] {
        let mut theirs = Vec::new();
        clap_complete::generate(clap, &mut probe(), "my-probe", &mut theirs);
        assert_eq!(
            script(shell, &mut probe(), "my-probe", &[]),
            String::from_utf8(theirs).unwrap(),
            "{shell:?}"
        );
    }
}

/// Beside the script in `scratch`: a Lua script, a folder with one in it,
/// and a film.
pub(crate) fn scripts_beside(scratch: &Scratch) {
    let folder = &scratch.folder;
    fs::create_dir(folder.join("scripts")).unwrap();
    for file in [
        "seek.lua",
        "scripts/next.lua",
        "film.mkv",
        "scripts/film.mkv",
    ] {
        fs::write(folder.join(file), b"").unwrap();
    }
}
