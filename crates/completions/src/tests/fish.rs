//! What fish offers with the scripts loaded, where it's installed, and its
//! quoting.

use std::fs;
use std::path::Path;

use super::text;
use crate::Shell;
use crate::tests::{AWKWARD, SCRIPTS, ask, scripts_beside, with_script};

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
    let scratch = with_script("fish", Shell::Fish, "my-probe.fish", &[]);
    fs::write(scratch.folder.join("film.mkv"), b"").unwrap();
    let offers = |line: &str| fish_offers(&scratch.folder, &scratch.script, line);
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
        text(AWKWARD)
    );
    if let Some(offers) = ask("fish", &folder, &["--no-config", "-c", &offer]) {
        assert_eq!(offers, [format!("word\t{AWKWARD}")]);
    }
}

#[test]
fn fish_offers_the_files_a_word_takes_besides_its_names_until_one_is_given() {
    let scratch = with_script("fish-files", Shell::Fish, "my-probe.fish", &[SCRIPTS]);
    scripts_beside(&scratch);
    let offers = |line: &str| fish_offers(&scratch.folder, &scratch.script, line);
    let Some(all) = offers("my-probe play ") else {
        return;
    };
    for offered in ["one\tThe first", "scripts/", "seek.lua"] {
        assert!(all.contains(&offered.to_owned()), "{offered}: {all:?}");
    }
    assert!(!all.contains(&"film.mkv".to_owned()), "{all:?}");
    assert_eq!(offers("my-probe play se").unwrap(), ["seek.lua"]);
    assert_eq!(
        offers("my-probe play scripts/").unwrap(),
        ["scripts/next.lua"]
    );
    // Nor names once a file's given: fish's own files then, as for any
    // word.
    let then = offers("my-probe play seek.lua ").unwrap();
    assert!(!then.iter().any(|word| word.contains('\t')), "{then:?}");
}
