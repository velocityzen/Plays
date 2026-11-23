//! What Nushell offers with the scripts loaded, as an editor asks it, where
//! it's installed, and the records its completers list, as text.

use std::fs;
use std::path::Path;

use clap::builder::PossibleValue;

use super::records;
use crate::Shell;
use crate::tests::{AWKWARD, SCRIPTS, ask, scripts_beside, with_script};

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
    let scratch = with_script("nushell", Shell::Nushell, "my-probe.nu", &[]);
    fs::write(scratch.folder.join("film.mkv"), b"").unwrap();
    let offers = |line: &str| nu_offers(&scratch.folder, line);
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
    let scratch = with_script("nushell-text", Shell::Nushell, "my-probe.nu", &[]);
    let text = fs::read_to_string(&scratch.script).unwrap();
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
    let records = records(vec![PossibleValue::new(AWKWARD).help(AWKWARD)]);
    let read = format!("let offered = ({records}); print $offered.0.value $offered.0.description");
    let folder = std::env::temp_dir();
    if let Some(printed) = ask("nu", &folder, &["--no-config-file", "-c", &read]) {
        assert_eq!(printed, [AWKWARD, AWKWARD]);
    }
}

#[test]
fn nushell_offers_the_files_a_word_takes_besides_its_names() {
    let scratch = with_script("nushell-files", Shell::Nushell, "my-probe.nu", &[SCRIPTS]);
    scripts_beside(&scratch);
    let text = fs::read_to_string(&scratch.script).unwrap();
    assert!(
        text.contains("def \"nu-complete my-probe play word\" [context: string] {"),
        "{text}"
    );
    assert!(
        text.contains("def \"nu-complete my-probe config get key\" [] {"),
        "{text}"
    );
    let offers = |line: &str| nu_offers(&scratch.folder, line);
    let Some(mut all) = offers("my-probe play ") else {
        return;
    };
    all.sort();
    assert_eq!(all, ["awkward", "bare", "one", "scripts/", "seek.lua"]);
    assert_eq!(offers("my-probe play se").unwrap(), ["seek.lua"]);
    assert_eq!(
        offers("my-probe play scripts/").unwrap(),
        ["scripts/next.lua"]
    );
    assert_eq!(
        offers("my-probe play nowhere/").unwrap(),
        Vec::<String>::new()
    );
}
