//! The scripts that complete the command line: what clap is told each
//! word can be, and what bash and fish offer with the scripts loaded, as
//! a terminal would ask them; zsh's is read for mistakes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Output};

use super::*;
use crate::cast::{airplay, chromecast, dlna};

/// A folder of the test's own, named for it, with the script for `shell`
/// in it, as `plays`.
fn with_script(name: &str, shell: Shell) -> (PathBuf, PathBuf) {
    let folder = std::env::temp_dir()
        .join("plays-tests")
        .join(format!("completions-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    let mut script = Vec::new();
    completions(&mut script, shell).unwrap();
    let path = folder.join("plays");
    fs::write(&path, script).unwrap();
    (folder, path)
}

/// Each line `output` printed, having printed nothing on stderr.
fn printed(output: &Output) -> Vec<String> {
    assert!(
        output.status.success() && output.stderr.is_empty(),
        "{output:?}"
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The names `command` offers for the word `arg` of its subcommand at
/// `path`, each with what it is.
fn offered(command: &Command, path: &[&str], arg: &str) -> Vec<(String, String)> {
    let mut command = command;
    for name in path {
        command = command.find_subcommand(name).unwrap();
    }
    let arg = command
        .get_arguments()
        .find(|found| found.get_id() == arg)
        .unwrap_or_else(|| panic!("{path:?} has no {arg}"));
    arg.get_possible_values()
        .iter()
        .map(|value| {
            let help = value.get_help().map(ToString::to_string);
            (value.get_name().to_owned(), help.unwrap_or_default())
        })
        .collect()
}

fn names(offered: &[(String, String)]) -> Vec<&str> {
    offered.iter().map(|(name, _)| name.as_str()).collect()
}

#[test]
fn the_words_that_name_commands_or_settings_are_told_the_names() {
    let command = command();
    let commands: Vec<(String, String)> = api::all()
        .map(|spec| (spec.id.to_owned(), spec.title.to_owned()))
        .collect();
    for path in [&["do"][..], &["api", "describe"], &["api", "call"]] {
        assert_eq!(offered(&command, path, "command"), commands, "{path:?}");
    }
    assert!(offered(&command, &["do"], "args").is_empty());
    assert!(offered(&command, &["api", "call"], "args").is_empty());

    let keys = offered(&command, &["settings", "get"], "key");
    assert_eq!(keys, offered(&command, &["settings", "set"], "key"));
    assert!(offered(&command, &["settings", "set"], "value").is_empty());
    let size = fields::find("subtitles.size", &[]).unwrap();
    assert!(
        keys.contains(&("subtitles.size".to_owned(), size.title.into_owned())),
        "{keys:?}"
    );
    for kind in [chromecast::KIND, dlna::KIND, airplay::KIND] {
        assert!(
            names(&keys).contains(&fields::look_with(kind).as_str()),
            "{kind}"
        );
    }
}

/// What bash offers for the last of `words`, with `script` loaded.
fn bash_offers(script: &Path, words: &[&str]) -> Vec<String> {
    const ASK: &str = r#"source "$1"; shift
COMP_WORDS=("$@"); COMP_CWORD=$(($# - 1)); COMP_LINE="$*"; COMP_POINT=${#COMP_LINE}
_plays plays "${COMP_WORDS[COMP_CWORD]}" "${COMP_WORDS[COMP_CWORD-1]}"
printf '%s\n' "${COMPREPLY[@]}""#;
    let output = Process::new("bash")
        .args(["--norc", "--noprofile", "-c", ASK, "bash"])
        .arg(script)
        .args(words)
        .output()
        .unwrap();
    printed(&output)
}

#[test]
fn bash_completes_subcommands_commands_ids_setting_keys_and_shells() {
    let (_, script) = with_script("bash", Shell::Bash);
    let offers = |words: &[&str]| bash_offers(&script, words);
    assert_eq!(offers(&["plays", "wi"]), ["window"]);
    let seeks = offers(&["plays", "do", "playback.seek"]);
    assert!(
        seeks.contains(&"playback.seek_forward".to_owned()),
        "{seeks:?}"
    );
    assert!(seeks.iter().all(|id| id.starts_with("playback.seek")));
    assert_eq!(offers(&["plays", "-w", "1", "do", "playback.seek"]), seeks);
    assert_eq!(
        offers(&["plays", "api", "describe", "app.qu"]),
        ["app.quit"]
    );
    assert_eq!(offers(&["plays", "api", "call", "app.qu"]), ["app.quit"]);
    assert_eq!(
        offers(&["plays", "settings", "get", "subtitles.si"]),
        ["subtitles.size"]
    );
    assert_eq!(
        offers(&["plays", "settings", "set", "casting.look_with."]).len(),
        3
    );
    assert_eq!(offers(&["plays", "completions", "f"]), ["fish"]);
}

/// What fish prints for `args`, in `folder`, or nothing if there's no fish
/// to ask.
fn fish(folder: &Path, args: &[&str]) -> Option<Vec<String>> {
    match Process::new("fish")
        .arg("--no-config")
        .args(args)
        .current_dir(folder)
        .output()
    {
        Ok(output) => Some(printed(&output)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            eprintln!("no fish to ask: what it offers goes unchecked");
            None
        }
        Err(err) => panic!("{err}"),
    }
}

#[test]
fn fish_completes_commands_ids_setting_keys_and_shells_and_still_files() {
    let (folder, script) = with_script("fish", Shell::Fish);
    fs::write(folder.join("film.mkv"), b"").unwrap();
    let script = script.to_str().unwrap();
    let offers = |line: &str| {
        fish(
            &folder,
            &["-c", "source $argv[1]; complete -C $argv[2]", script, line],
        )
    };
    let Some(seeks) = offers("plays do playback.seek") else {
        return;
    };
    assert!(
        seeks.contains(&"playback.seek\tSeek To".to_owned()),
        "{seeks:?}"
    );
    assert!(seeks.iter().all(|id| id.starts_with("playback.seek")));
    assert_eq!(offers("plays -w 1 do playback.seek"), Some(seeks));
    let given = offers("plays do playback.seek ").unwrap();
    assert!(!given.iter().any(|id| id.contains('\t')), "{given:?}");

    let quit = Some(vec!["app.quit\tQuit Plays".to_owned()]);
    assert_eq!(offers("plays api describe app.qu"), quit);
    assert_eq!(offers("plays api call app.qu"), quit);
    let listed = offers("plays api list ").unwrap();
    assert!(
        !listed.iter().any(|id| id.starts_with("app.")),
        "{listed:?}"
    );

    let keys = offers("plays settings get subtitles.si").unwrap();
    assert_eq!(keys.len(), 1, "{keys:?}");
    assert!(keys[0].starts_with("subtitles.size\t"), "{keys:?}");
    assert_eq!(
        offers("plays settings set casting.look_with.")
            .unwrap()
            .len(),
        3
    );

    let mut shells = offers("plays completions ").unwrap();
    shells.sort();
    assert_eq!(shells, ["bash", "fish", "zsh"]);
    assert_eq!(offers("plays open fi"), Some(vec!["film.mkv".to_owned()]));
}

#[test]
fn fish_reads_what_it_offers_as_it_is() {
    let (folder, _) = with_script("fish-text", Shell::Fish);
    let text = r#"it's "quoted", $HOME, a back\slash and two \\"#;
    let offer = format!(
        "complete -c probe -f -a \"word\\t'{}'\"; complete -C 'probe '",
        fish_text(text)
    );
    if let Some(offers) = fish(&folder, &["-c", &offer]) {
        assert_eq!(offers, [format!("word\t{text}")]);
    }
}

#[test]
fn zsh_reads_its_script_and_it_offers_the_commands_ids_and_files() {
    let (_, script) = with_script("zsh", Shell::Zsh);
    let output = Process::new("zsh")
        .args(["--no-rcs", "-n"])
        .arg(&script)
        .output()
        .unwrap();
    printed(&output);
    let text = fs::read_to_string(&script).unwrap();
    assert!(text.starts_with("#compdef plays"));
    // What each such word offers, from its name to the end of its names.
    let offers = |word: &str| -> Vec<&str> {
        text.match_indices(word)
            .map(|(at, _)| {
                let rest = &text[at..];
                &rest[..rest.find("))'").unwrap_or_default()]
            })
            .collect()
    };
    let commands = offers("':command");
    assert_eq!(commands.len(), 3, "do, api describe and api call");
    for names in commands {
        assert!(names.contains(r#"playback.seek\:"Seek To""#), "{names}");
    }
    let keys = offers("':key:");
    assert_eq!(keys.len(), 2, "settings get and set");
    for names in keys {
        assert!(names.contains(r"subtitles.size\:"), "{names}");
    }
    assert!(text.contains("':shell:(bash zsh fish)'"));
    assert!(text.contains("'*::paths:_files'"));
    assert!(!text.contains("complete -c plays"), "fish's lines");
}
