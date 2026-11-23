//! The scripts that complete a clap command line in bash, zsh, fish and
//! Nushell: clap's own (`clap_complete`, `clap_complete_nushell`), with
//! what they leave out filled in. In fish, clap's completes options alone,
//! so lines are added offering the names a word takes (the possible values
//! of a positional argument), each with what it is, until one is given.
//! In Nushell, clap's completers list the names alone, so each lists
//! records instead, a name with what it is. A word may take files too,
//! besides its names (`Files`): those ending in a suffix, and folders to
//! find them in.

use std::fmt::Write as _;

use clap::Command;
use clap::builder::PossibleValue;
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
            .for_each(|files| bash_files(&mut script, files)),
        Shell::Zsh => files
            .iter()
            .for_each(|files| zsh_files(&mut script, command, bin, files)),
        Shell::Fish => script += &fish_words(command, bin, files),
        Shell::Nushell => {
            script = nu_described(&script, command);
            files
                .iter()
                .for_each(|files| nu_files(&mut script, command, files));
        }
    }
    script
}

/// The subcommand at `path` within `command`, and its first word.
fn first_word<'a>(command: &'a Command, path: &[&str]) -> Option<(&'a Command, &'a str)> {
    let mut command = command;
    for name in path {
        command = command.find_subcommand(name)?;
    }
    Some((command, command.get_positionals().next()?.get_id().as_str()))
}

/// Have bash offer `files` for their word, besides its names, in
/// `script`, clap's.
fn bash_files(script: &mut String, files: &Files) {
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

/// Have zsh offer `files` for their word, besides its names, in `script`,
/// clap's: the word offers both through a function of its own.
fn zsh_files(script: &mut String, command: &Command, bin: &str, files: &Files) {
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

/// What clap's script for fish leaves out, as it completes options
/// alone: the words with names to offer, each offered until one of its
/// names is given, or a file of those `files` offer.
fn fish_words(command: &Command, bin: &str, files: &[Files]) -> String {
    let mut lines = String::from("\n# The words that aren't options, and the names each takes.\n");
    if !files.is_empty() {
        // The folders and the files ending in the suffix given, of those
        // in the folder of the word typed.
        writeln!(
            lines,
            "function {} -a suffix\n    \
             for file in (commandline -ct)*\n        \
             if test -d $file\n            echo $file/\n        \
             else if string match -q -- \"*$suffix\" $file\n            echo $file\n        \
             end\n    end\nend",
            fish_files_ending(bin)
        )
        .ok();
    }
    for sub in command.get_subcommands() {
        fish_offer_within(&mut lines, bin, &[sub.get_name()], sub, files);
    }
    lines
}

/// The name of the function listing files for a word that takes them.
fn fish_files_ending(bin: &str) -> String {
    format!("__fish_{}_files_ending", bin.replace('-', "_"))
}

/// The lines offering the names the words of `command`, and of each
/// subcommand within it, take; `path` being the subcommands that name it.
fn fish_offer_within(
    lines: &mut String,
    bin: &str,
    path: &[&str],
    command: &Command,
    files: &[Files],
) {
    fish_offer(lines, bin, path, command, files);
    for nested in command.get_subcommands() {
        let within = [path, &[nested.get_name()]].concat();
        fish_offer_within(lines, bin, &within, nested, files);
    }
}

/// The lines offering the names `command`'s words take, and the files its
/// first takes if `files` has it take some, `path` being the subcommands
/// that name it.
fn fish_offer(lines: &mut String, bin: &str, path: &[&str], command: &Command, files: &[Files]) {
    let [first, within @ ..] = path else {
        return;
    };
    let suffix = files
        .iter()
        .find(|files| files.path == path)
        .map(|files| files.suffix);
    for (at, arg) in command.get_positionals().enumerate() {
        let values = arg.get_possible_values();
        if values.is_empty() {
            continue;
        }
        // As clap names it in its script.
        let using = format!("__fish_{}_using_subcommand", bin.replace('-', "_"));
        let mut when = format!("{using} {first}");
        for word in within {
            write!(when, "; and __fish_seen_subcommand_from {word}").ok();
        }
        let names: Vec<&str> = values.iter().map(PossibleValue::get_name).collect();
        write!(
            when,
            "; and not __fish_seen_subcommand_from {}",
            names.join(" ")
        )
        .ok();
        // Nor once a file it takes is given.
        let suffix = suffix.filter(|_| at == 0).map(fish_text);
        if let Some(suffix) = &suffix {
            write!(
                when,
                "; and not string match -q -- '*{suffix}' (commandline -opc)"
            )
            .ok();
        }
        let offered: Vec<String> = values
            .iter()
            .map(|value| {
                let help = value
                    .get_help()
                    .map(ToString::to_string)
                    .unwrap_or_default();
                format!("{}\\t'{}'", fish_text(value.get_name()), fish_text(&help))
            })
            .collect();
        writeln!(
            lines,
            "complete -c {bin} -n \"{when}\" -f -a \"{}\"",
            offered.join("\n")
        )
        .ok();
        if let Some(suffix) = suffix {
            let listed = fish_files_ending(bin);
            writeln!(
                lines,
                "complete -c {bin} -n \"{when}\" -f -a \"({listed} '{suffix}')\""
            )
            .ok();
        }
    }
}

/// `text` to be read as it is from between single quotes within double
/// ones, as fish reads what `complete -a` offers: first as the double
/// quotes are read, then the single ones.
fn fish_text(text: &str) -> String {
    text.replace('\\', "\\\\\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

/// `script`, clap's for Nushell, with what it leaves out: what each name
/// it offers is beside it. Its completers list the names alone; each lists
/// records instead.
fn nu_described(script: &str, command: &Command) -> String {
    let mut script = script.to_owned();
    let mut commands = vec![command];
    while let Some(command) = commands.pop() {
        commands.extend(command.get_subcommands());
        let bin = command.get_bin_name().unwrap_or(command.get_name());
        for arg in command.get_arguments() {
            let head = format!("def \"nu-complete {bin} {}\" [] {{", arg.get_id());
            let Some(at) = script.find(&head).map(|at| at + head.len()) else {
                continue;
            };
            if let Some(end) = script[at..].find("\n  }") {
                script.replace_range(at..at + end, &nu_records(arg.get_possible_values()));
            }
        }
    }
    script
}

/// Have Nushell offer `files` for their word, besides its names, in
/// `script`: its completer lists them after its names, from the folder of
/// the word typed, which it's given.
fn nu_files(script: &mut String, command: &Command, files: &Files) {
    let Some((command, word)) = first_word(command, files.path) else {
        return;
    };
    let bin = command.get_bin_name().unwrap_or(command.get_name());
    let head = format!("def \"nu-complete {bin} {word}\" [] {{");
    let Some(start) = script.find(&head) else {
        return;
    };
    let Some(end) = script[start..].find("\n  }").map(|end| start + end) else {
        return;
    };
    let records = script[start + head.len()..end].to_owned();
    let head = head.replace(" [] {", " [context: string] {");
    let suffix = nu_text(files.suffix);
    let completer = format!(
        "{head}\n    let typed = $context | split row \" \" | last\n    \
         let folder = $typed | str replace --regex '[^/]*$' ''\n    \
         let listed = try {{ ls (if $folder == \"\" {{ \".\" }} else {{ $folder }}) }} \
         catch {{ [] }}{records} | append (\n      $listed\n      \
         | where {{|file| $file.type == dir or ($file.name | str ends-with \"{suffix}\") }}\n      \
         | each {{|file| if $file.type == dir {{ {{ value: $\"($file.name)/\" }} }} \
         else {{ {{ value: $file.name }} }} }}\n    )"
    );
    script.replace_range(start..end, &completer);
}

/// `values` as a completer's list of records, each with what it is where
/// it says.
fn nu_records(values: Vec<PossibleValue>) -> String {
    let mut records = String::from("\n    [\n");
    for value in values {
        write!(records, "      {{ value: \"{}\"", nu_text(value.get_name())).ok();
        if let Some(help) = value.get_help() {
            write!(records, ", description: \"{}\"", nu_text(&help.to_string())).ok();
        }
        records.push_str(" }\n");
    }
    records.push_str("    ]");
    records
}

/// `text` to be read as it is from between double quotes, as Nushell reads
/// a string: backslashes and quotes escaped. Nothing in one is run.
fn nu_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
#[path = "tests/completions.rs"]
mod tests;
