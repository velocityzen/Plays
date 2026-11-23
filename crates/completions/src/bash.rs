//! bash: clap's script, with the words that take files offering them too.

use crate::Files;

/// Have bash offer `files` for their word, besides its names, in
/// `script`, clap's.
pub(crate) fn with_files(script: &mut String, files: &Files) {
    // The case for the subcommand, named as clap names it as it reads the
    // words: the command, then each subcommand within it.
    let named = |within: &str| {
        let head = format!("            {within})\n                cmd=\"");
        let at = script.find(&head)? + head.len();
        Some(script[at..at + script[at..].find('"')?].to_owned())
    };
    let Some(mut cmd) = named("\",$1\"") else {
        return;
    };
    for name in files.path {
        let Some(within) = named(&format!("{cmd},{name}")) else {
            return;
        };
        cmd = within;
    }
    let Some(at) = script.find(&format!("\n        {cmd})\n")) else {
        return;
    };
    // Where its first word is offered its names.
    let names = "                COMPREPLY=( $(compgen -W \"${opts}\" -- \"${cur}\") )\n";
    let Some(offered) = script[at..]
        .find(names)
        .map(|found| at + found + names.len())
    else {
        return;
    };
    let suffix = files.suffix.replace('\'', r"'\''");
    let lines = format!(
        "                compopt -o filenames 2> /dev/null\n                \
         COMPREPLY+=( $(compgen -d -- \"${{cur}}\") \
         $(compgen -f -X '!*{suffix}' -- \"${{cur}}\") )\n"
    );
    script.insert_str(offered, &lines);
}

#[cfg(test)]
#[path = "tests/bash.rs"]
mod tests;
