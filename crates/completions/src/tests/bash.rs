//! What bash offers with the scripts loaded, where it's installed.

use std::fs;
use std::path::Path;

use crate::tests::{SCRIPTS, ask, probe, scripts_beside, with_script};
use crate::{Shell, script};

/// What bash offers for the last of `words`, in `folder`, with `script`
/// loaded, written for the command line run as `probe`.
fn bash_offers(folder: &Path, script: &Path, words: &[&str]) -> Option<Vec<String>> {
    const ASK: &str = r#"source "$1"; shift
COMP_WORDS=("$@"); COMP_CWORD=$(($# - 1)); COMP_LINE="$*"; COMP_POINT=${#COMP_LINE}
_probe probe "${COMP_WORDS[COMP_CWORD]}" "${COMP_WORDS[COMP_CWORD-1]}"
printf '%s\n' "${COMPREPLY[@]}""#;
    let script = script.to_str().unwrap();
    let args = [
        &["--norc", "--noprofile", "-c", ASK, "bash", script][..],
        words,
    ]
    .concat();
    let mut offers = ask("bash", folder, &args)?;
    offers.sort();
    Some(offers)
}

#[test]
fn bash_offers_the_files_a_word_takes_besides_its_names() {
    let scratch = with_script("bash-files", Shell::Bash, "probe.bash", &[SCRIPTS]);
    // As `probe`: clap's script for a name with a dash never reaches its
    // subcommands, naming their cases apart from what it reads.
    fs::write(
        &scratch.script,
        script(Shell::Bash, &mut probe(), "probe", &[SCRIPTS]),
    )
    .unwrap();
    scripts_beside(&scratch);
    let offers = |words: &[&str]| bash_offers(&scratch.folder, &scratch.script, words);
    let Some(all) = offers(&["probe", "play", ""]) else {
        return;
    };
    for offered in ["one", "awkward", "scripts", "seek.lua"] {
        assert!(all.contains(&offered.to_owned()), "{offered}: {all:?}");
    }
    assert!(!all.contains(&"film.mkv".to_owned()), "{all:?}");
    assert_eq!(
        offers(&["probe", "play", "s"]).unwrap(),
        ["scripts", "seek.lua"]
    );
    assert_eq!(
        offers(&["probe", "play", "scripts/"]).unwrap(),
        ["scripts/next.lua"]
    );
    assert_eq!(offers(&["probe", "play", "-"]).unwrap(), ["--help", "-h"]);

    fs::write(scratch.folder.join("zz seek.lua"), b"").unwrap();
    fs::create_dir(scratch.folder.join("zz scripts")).unwrap();
    assert_eq!(
        offers(&["probe", "play", "zz"]).unwrap(),
        ["zz scripts", "zz seek.lua"],
        "a name with a space, whole"
    );
}
