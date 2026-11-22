//! `cli`: the command line, put where a terminal finds it.

use std::fmt;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use gpui::{App, ClipboardItem, PromptButton, PromptLevel};
use serde_json::Value;

use crate::api::registry::Spec;
use crate::api::{Error, Source};
use crate::settings::Settings;

/// Where the command line goes: on the PATH a terminal has to start with.
const FOLDER: &str = "/usr/local/bin";

#[cfg(test)]
thread_local! {
    /// A folder of a test's own, in place of `FOLDER`.
    static TEST_FOLDER: std::cell::RefCell<Option<PathBuf>> = const {
        std::cell::RefCell::new(None)
    };
}

/// `FOLDER`, or in a test, the folder it gives.
fn folder() -> PathBuf {
    #[cfg(test)]
    if let Some(folder) = TEST_FOLDER.with_borrow(Clone::clone) {
        return folder;
    }
    PathBuf::from(FOLDER)
}

pub const COMMANDS: &[Spec] = &[Spec::app_with(
    "cli.install",
    "Install Command Line Tool",
    "Link plays into /usr/local/bin, so a terminal can run it: to open files, run \
     commands and hear what changes.",
    &[],
    |args, cx| {
        let installed = install();
        tell(args.source, &installed, cx);
        match installed {
            Ok(link) => Ok(Value::from(link.display().to_string())),
            Err(cant) => Err(Error::failed(cant.to_string())),
        }
    },
)
.keywords(&["terminal", "shell", "path", "command line"])
// Elsewhere, Plays is installed where the system keeps programs.
.only_if(|| cfg!(target_os = "macos"))
.returns("The link's path.")];

/// Why the command line can't be linked.
#[derive(Clone, Debug, PartialEq)]
enum Cant {
    /// Plays can't find itself.
    Lost(String),
    /// Something that isn't a link is where the link goes.
    Taken(PathBuf),
    /// Plays can't write the link to `program` at `link`, `why`; someone
    /// allowed to, typing a command, can, making its folder too if it
    /// `folder_missing`.
    Denied {
        program: PathBuf,
        link: PathBuf,
        folder_missing: bool,
        why: String,
    },
}

impl Cant {
    /// What makes the link from a terminal, if typing something can.
    fn command(&self) -> Option<String> {
        let Self::Denied {
            program,
            link,
            folder_missing,
            ..
        } = self
        else {
            return None;
        };
        let link_it = format!("sudo ln -sf '{}' {}", program.display(), link.display());
        let folder = link.parent().filter(|_| *folder_missing);
        Some(match folder {
            Some(folder) => format!("sudo mkdir -p {} && {link_it}", folder.display()),
            None => link_it,
        })
    }
}

impl fmt::Display for Cant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lost(why) => write!(f, "can't find Plays itself: {why}"),
            Self::Taken(link) => write!(
                f,
                "{} is there already, and isn't a link: move it away first",
                link.display()
            ),
            Self::Denied { link, why, .. } => {
                write!(f, "can't link {}: {why}", link.display())?;
                if let Some(command) = self.command() {
                    write!(f, ". In a terminal: {command}")?;
                }
                Ok(())
            }
        }
    }
}

/// Link Plays, this program, into `FOLDER`.
fn install() -> Result<PathBuf, Cant> {
    let program = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|err| Cant::Lost(err.to_string()))?;
    link(&program, &folder())
}

/// Link `program` into `folder` as `plays`, in place of a link there but
/// of nothing else; if `folder` can't be written to, say what to type in
/// a terminal instead.
fn link(program: &Path, folder: &Path) -> Result<PathBuf, Cant> {
    let link = folder.join("plays");
    match fs::symlink_metadata(&link) {
        Ok(found) if !found.is_symlink() => return Err(Cant::Taken(link)),
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(denied(program, &link, &err)),
    }
    // Made beside it and moved over it, so a link there stays until the
    // new one replaces it.
    let made = folder.join(format!(".plays-{}", std::process::id()));
    fs::remove_file(&made).ok();
    symlink(program, &made).map_err(|err| denied(program, &link, &err))?;
    if let Err(err) = fs::rename(&made, &link) {
        fs::remove_file(&made).ok();
        return Err(denied(program, &link, &err));
    }
    Ok(link)
}

/// `link` can't be made, as `err` says.
fn denied(program: &Path, link: &Path, err: &io::Error) -> Cant {
    Cant::Denied {
        program: program.to_owned(),
        link: link.to_owned(),
        folder_missing: link.parent().is_some_and(|folder| !folder.is_dir()),
        why: err.to_string(),
    }
}

/// Say in an alert, over the window in front, what installing came to:
/// where the link is, or why there's none, with the command that makes it,
/// which the alert copies. Not when a command line ran it, which prints it.
fn tell(source: Source, installed: &Result<PathBuf, Cant>, cx: &mut App) {
    if source == Source::Cli {
        return;
    }
    let Some(window) = cx
        .active_window()
        .or_else(|| cx.windows().into_iter().next())
    else {
        return;
    };
    let (level, message, detail, command) = match installed {
        Ok(link) => {
            let mut detail = format!("A terminal runs it as plays, from {}.", link.display());
            if !cx.global::<Settings>().command_line_control {
                detail.push_str(
                    " For it to run commands and hear what changes, turn on Allow \
                     control from the command line in Settings → Command Line.",
                );
            }
            let message = "The command line tool is installed";
            (PromptLevel::Info, message, detail, None)
        }
        Err(cant) => {
            let command = cant.command();
            let detail = match (cant, &command) {
                (Cant::Denied { link, why, .. }, Some(command)) => format!(
                    "Plays can't link {}: {why}. In a terminal, this makes the link, \
                     asking for your password:\n\n{command}",
                    link.display()
                ),
                _ => format!("{}.", capitalized(&cant.to_string())),
            };
            let message = "The command line tool can't be installed";
            (PromptLevel::Warning, message, detail, command)
        }
    };
    let buttons = match command {
        Some(_) => vec![PromptButton::new("Copy Command"), PromptButton::ok("OK")],
        None => vec![PromptButton::ok("OK")],
    };
    // Once the window is free: it may be the one updating now.
    cx.defer(move |cx| {
        let shown = window.update(cx, |_, window, cx| {
            window.prompt(level, message, Some(&detail), &buttons[..], cx)
        });
        let Ok(answer) = shown else {
            return;
        };
        cx.spawn(async move |cx| {
            if let (Ok(0), Some(command)) = (answer.await, command) {
                cx.update(|cx| cx.write_to_clipboard(ClipboardItem::new_string(command)))
                    .ok();
            }
        })
        .detach();
    });
}

/// `text` with a capital at the start, as an alert's sentences have.
fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "tests/cli.rs"]
mod tests;
