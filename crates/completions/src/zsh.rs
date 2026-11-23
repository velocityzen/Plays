//! zsh: clap's script, with the words that take files offering them too.

use clap::Command;

use crate::{Files, first_word};

/// Have zsh offer `files` for their word, besides its names, in `script`,
/// clap's: the word offers both through a function of its own.
pub(crate) fn with_files(script: &mut String, command: &Command, bin: &str, files: &Files) {
    let Some((_, word)) = first_word(command, files.path) else {
        return;
    };
    // The subcommand's case, each within the one before.
    let mut at = 0;
    for name in files.path {
        let case = format!("({name})\n");
        let Some(found) = script[at..].find(&case) else {
            return;
        };
        at += found + case.len();
    }
    let Some(spec) = [format!("\n'::{word}"), format!("\n':{word}")]
        .iter()
        .find_map(|head| script[at..].find(head.as_str()))
        .map(|found| at + found)
    else {
        return;
    };
    let line = spec..spec + script[spec..].find(" \\\n").unwrap_or_default();
    let (Some(start), Some(end)) = (
        script[line.clone()].find(":(("),
        script[line.clone()].rfind("))'"),
    ) else {
        return;
    };
    let (start, end) = (line.start + start + 1, line.start + end + 2);
    let names = script[start..end].to_owned();
    let function = format!("_{bin}__subcmd__{}__{word}", files.path.join("__subcmd__"));
    script.replace_range(start..end, &function);
    let suffix = files.suffix.replace('\'', r"'\''").replace('"', "\\\"");
    let defined = format!(
        "(( $+functions[{function}] )) ||\n{function}() {{\n    _alternative \\\n        \
         'names:{word}:{names}' \\\n        'files:file:_files -g \"*{suffix}\"'\n}}\n\n"
    );
    let last = format!("if [ \"$funcstack[1]\" = \"_{bin}\" ]; then");
    let at = script.rfind(&last).unwrap_or(script.len());
    script.insert_str(at, &defined);
}

#[cfg(test)]
#[path = "tests/zsh.rs"]
mod tests;
