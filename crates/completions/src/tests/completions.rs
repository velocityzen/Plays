//! The scripts for a small command line of the tests' own: what fish and
//! Nushell offer with them loaded, as a terminal or an editor would ask,
//! where those shells are installed, and their quoting as text.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Output};

use clap::builder::PossibleValuesParser;
use clap::{Arg, ValueHint};

use super::*;

/// What its names say, as no shell should read it: quotes, a variable,
/// a command to run and backslashes.
const AWKWARD: &str = r#"it's "quoted", $HOME, (not run), a back\slash and two \\"#;

/// The command line the tests complete, run as `my-probe`: `play` takes a
/// word with names, `config get` a key, nested a level down, and `open`
/// files.
fn probe() -> Command {
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
        .subcommand(Command::new("play").arg(Arg::new("word").value_parser(words)))
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

/// A folder of the test's own, named for it, with the script for `shell`
/// in it as `file`.
fn with_script(name: &str, shell: Shell, file: &str) -> (PathBuf, PathBuf) {
    let folder = std::env::temp_dir()
        .join("completions-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    let path = folder.join(file);
    fs::write(&path, script(shell, &mut probe(), "my-probe")).unwrap();
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

/// What `shell` prints for `args`, in `folder`, or nothing if there's no
/// such shell to ask.
fn ask(shell: &str, folder: &Path, args: &[&str]) -> Option<Vec<String>> {
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
            script(shell, &mut probe(), "my-probe"),
            String::from_utf8(theirs).unwrap(),
            "{shell:?}"
        );
    }
}

/// What fish offers for the end of `line`, with the script in `folder`
/// loaded.
fn fish_offers(folder: &Path, script: &Path, line: &str) -> Option<Vec<String>> {
    let ask_it = "source $argv[1]; complete -C $argv[2]";
    let script = script.to_str().unwrap();
    let mut offers = ask("fish", folder, &["--no-config", "-c", ask_it, script, line])?;
    offers.sort();
    Some(offers)
}

#[test]
fn fish_offers_the_names_a_word_takes_until_one_is_given() {
    let (folder, script) = with_script("fish", Shell::Fish, "my-probe.fish");
    fs::write(folder.join("film.mkv"), b"").unwrap();
    let offers = |line: &str| fish_offers(&folder, &script, line);
    let Some(words) = offers("my-probe play ") else {
        return;
    };
    assert_eq!(
        words,
        [
            "awkward\t".to_owned() + AWKWARD,
            "bare".into(),
            "one\tThe first".into()
        ]
    );
    assert_eq!(offers("my-probe play o").unwrap(), ["one\tThe first"]);
    let given = offers("my-probe play one ").unwrap();
    assert!(!given.iter().any(|word| word.contains('\t')), "{given:?}");
    assert_eq!(
        offers("my-probe config get size.").unwrap(),
        ["size.large\tLarge", "size.small\tSmall"]
    );
    // The keys are `get`'s: not offered before it is given.
    let within = offers("my-probe config ").unwrap();
    assert!(
        within.iter().any(|word| word.starts_with("get")),
        "{within:?}"
    );
    assert!(
        !within.iter().any(|word| word.starts_with("size.")),
        "{within:?}"
    );
    assert_eq!(offers("my-probe open fi").unwrap(), ["film.mkv"]);
    assert_eq!(offers("my-probe pl").unwrap(), ["play"]);
}

#[test]
fn fish_reads_what_it_offers_as_it_is() {
    let folder = std::env::temp_dir();
    let offer = format!(
        "complete -c probe -f -a \"word\\t'{}'\"; complete -C 'probe '",
        fish_text(AWKWARD)
    );
    if let Some(offers) = ask("fish", &folder, &["--no-config", "-c", &offer]) {
        assert_eq!(offers, [format!("word\t{AWKWARD}")]);
    }
}

/// What Nushell offers for the end of `line`, in `folder`, with the
/// script there as `my-probe.nu` and used, as `use my-probe.nu *` in its
/// config would; as an editor asks it.
fn nu_offers(folder: &Path, line: &str) -> Option<Vec<String>> {
    let typed = format!("use my-probe.nu *\n{line}");
    fs::write(folder.join("typed.nu"), &typed).unwrap();
    let at = typed.len().to_string();
    let args = ["--no-config-file", "--ide-complete", &at, "typed.nu"];
    let printed = ask("nu", folder, &args)?;
    let listed: serde_json::Value = serde_json::from_str(&printed.concat()).unwrap();
    let offered = listed["completions"].as_array().unwrap().iter();
    Some(
        offered
            .map(|name| name.as_str().unwrap().to_owned())
            .collect(),
    )
}

#[test]
fn nushell_offers_the_names_a_word_takes() {
    let (folder, _) = with_script("nushell", Shell::Nushell, "my-probe.nu");
    fs::write(folder.join("film.mkv"), b"").unwrap();
    let offers = |line: &str| nu_offers(&folder, line);
    let Some(mut words) = offers("my-probe play ") else {
        return;
    };
    words.sort();
    assert_eq!(words, ["awkward", "bare", "one"]);
    assert_eq!(offers("my-probe play o").unwrap(), ["one"]);
    assert_eq!(
        offers("my-probe config get size.s").unwrap(),
        ["size.small"]
    );
    assert_eq!(offers("my-probe open fi").unwrap(), ["film.mkv"]);
}

#[test]
fn nushell_lists_each_name_with_what_it_is() {
    let (_, script) = with_script("nushell-text", Shell::Nushell, "my-probe.nu");
    let text = fs::read_to_string(script).unwrap();
    assert!(text.starts_with("module completions {"), "{text}");
    assert!(text.ends_with("export use completions *\n"));
    let quoted = r#"it's \"quoted\", $HOME, (not run), a back\\slash and two \\\\"#;
    let words = format!(
        r#"def "nu-complete my-probe play word" [] {{
    [
      {{ value: "one", description: "The first" }}
      {{ value: "awkward", description: "{quoted}" }}
      {{ value: "bare" }}
    ]
  }}"#
    );
    assert!(text.contains(&words), "{text}");
    let keys = r#"def "nu-complete my-probe config get key" [] {
    [
      { value: "size.small", description: "Small" }
      { value: "size.large", description: "Large" }
    ]
  }"#;
    assert!(text.contains(keys), "{text}");
}

#[test]
fn nushell_reads_what_it_offers_as_it_is() {
    let records = nu_records(vec![PossibleValue::new(AWKWARD).help(AWKWARD)]);
    let read = format!("let offered = ({records}); print $offered.0.value $offered.0.description");
    let folder = std::env::temp_dir();
    if let Some(printed) = ask("nu", &folder, &["--no-config-file", "-c", &read]) {
        assert_eq!(printed, [AWKWARD, AWKWARD]);
    }
}
