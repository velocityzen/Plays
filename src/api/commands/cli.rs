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
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use gpui::TestAppContext;

    use super::*;
    use crate::settings::Settings;
    use crate::ui::testing;

    /// A folder of the test's own, empty, removed as the test ends, pass
    /// or fail, even shut.
    struct Folder(PathBuf);

    impl std::ops::Deref for Folder {
        type Target = Path;

        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700)).ok();
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn folder(name: &str) -> Folder {
        let folder = std::env::temp_dir()
            .join("plays-tests")
            .join(format!("cli-{name}-{}", std::process::id()));
        fs::remove_dir_all(&folder).ok();
        fs::create_dir_all(&folder).unwrap();
        Folder(folder)
    }

    #[test]
    fn plays_is_linked_where_a_terminal_finds_it() {
        let folder = folder("link");
        let program = Path::new("/Applications/Plays.app/Contents/MacOS/plays");
        let link = link(program, &folder).unwrap();
        assert_eq!(link, folder.join("plays"));
        assert_eq!(fs::read_link(&link).unwrap(), program);

        let moved = Path::new("/Users/me/Applications/Plays.app/Contents/MacOS/plays");
        link_again(moved, &folder);
        let names: Vec<_> = fs::read_dir(&*folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["plays"], "nothing left beside it");
    }

    /// Link `program` where a link is already, which it replaces.
    fn link_again(program: &Path, folder: &Path) {
        let link = link(program, folder).unwrap();
        assert_eq!(fs::read_link(link).unwrap(), program, "the link replaced");
    }

    #[test]
    fn a_tests_folder_is_removed_as_it_ends() {
        let shut = folder("gone");
        let path = shut.to_path_buf();
        fs::write(path.join("plays"), "").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o500)).unwrap();
        drop(shut);
        assert!(!path.exists(), "{} is left", path.display());
    }

    #[test]
    fn what_isnt_a_link_is_left_there() {
        let folder = folder("file");
        fs::write(folder.join("plays"), "another program").unwrap();
        let err = link(Path::new("/Applications/Plays.app/plays"), &folder).unwrap_err();
        assert!(err.to_string().contains("isn't a link"), "{err}");
        assert_eq!(
            fs::read_to_string(folder.join("plays")).unwrap(),
            "another program"
        );
    }

    #[test]
    fn a_folder_that_isnt_there_is_made_too() {
        let folder = folder("missing");
        let missing = folder.join("bin");
        let program = Path::new("/Applications/Plays.app/Contents/MacOS/plays");
        let cant = link(program, &missing).unwrap_err();
        assert_eq!(
            cant.command().as_deref(),
            Some(
                format!(
                    "sudo mkdir -p {} && sudo ln -sf '{}' {}",
                    missing.display(),
                    program.display(),
                    missing.join("plays").display()
                )
                .as_str()
            )
        );
        assert!(!missing.exists(), "made by whoever types it, not by Plays");
    }

    fn denied() -> Cant {
        Cant::Denied {
            program: "/Applications/Plays.app/Contents/MacOS/plays".into(),
            link: "/usr/local/bin/plays".into(),
            folder_missing: false,
            why: "Permission denied (os error 13)".into(),
        }
    }

    #[cfg(target_os = "macos")]
    #[gpui::test]
    fn installing_says_so_in_an_alert_unless_a_command_line_ran_it(cx: &mut TestAppContext) {
        use crate::api::{self, Invocation};

        let (_, mut cx) = testing::start(cx);
        let folder = folder("installed");
        TEST_FOLDER.set(Some(folder.to_path_buf()));
        let program = std::env::current_exe().unwrap().canonicalize().unwrap();
        let link = folder.join("plays").display().to_string();
        let install = |source, cx: &mut TestAppContext| {
            let invocation = Invocation::new("cli.install").from(source);
            let ran = TestAppContext::update(cx, |cx| api::run(invocation, cx));
            cx.run_until_parked();
            ran
        };

        assert_eq!(install(Source::Cli, &mut cx), Ok(Value::from(link.clone())));
        assert_eq!(fs::read_link(folder.join("plays")).unwrap(), program);
        assert!(!cx.has_pending_prompt(), "the terminal says it");
        assert_eq!(install(Source::Action, &mut cx), Ok(Value::from(link)));
        let (message, _) = cx.pending_prompt().expect("an alert, from the menu");
        assert_eq!(message, "The command line tool is installed");
        cx.simulate_prompt_answer("OK");
        TEST_FOLDER.set(None);
    }

    #[gpui::test]
    fn says_what_it_did_in_an_alert_unless_a_command_line_ran_it(cx: &mut TestAppContext) {
        let (_, mut cx) = testing::start(cx);
        let link = PathBuf::from("/usr/local/bin/plays");
        let installed = Ok(link.clone());
        cx.update(|_, cx| tell(Source::Cli, &installed, cx));
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt(), "the terminal says it");

        cx.update(|_, cx| tell(Source::Action, &installed, cx));
        cx.run_until_parked();
        let (message, detail) = cx.pending_prompt().expect("an alert");
        assert_eq!(message, "The command line tool is installed");
        assert!(
            detail.starts_with("A terminal runs it as plays, from /usr/local/bin/plays.")
                && detail.ends_with(
                    "turn on Allow control from the command line in Settings → Command Line."
                ),
            "{detail}"
        );
        cx.simulate_prompt_answer("OK");

        cx.update(|_, cx| Settings::update(cx, false, |s| s.command_line_control = true));
        cx.update(|_, cx| tell(Source::Palette, &installed, cx));
        cx.run_until_parked();
        let (_, detail) = cx.pending_prompt().expect("an alert");
        assert_eq!(
            detail, "A terminal runs it as plays, from /usr/local/bin/plays.",
            "control is on already"
        );
        cx.simulate_prompt_answer("OK");
    }

    #[gpui::test]
    fn an_alert_copies_the_command_that_installs_it_where_plays_cant(cx: &mut TestAppContext) {
        let (_, mut cx) = testing::start(cx);
        let command = denied().command().unwrap();
        cx.update(|_, cx| tell(Source::Ui, &Err(denied()), cx));
        cx.run_until_parked();
        let (message, detail) = cx.pending_prompt().expect("an alert");
        assert_eq!(message, "The command line tool can't be installed");
        assert!(
            detail.contains("Permission denied") && detail.ends_with(&command),
            "{detail}"
        );
        cx.simulate_prompt_answer("Copy Command");
        cx.run_until_parked();
        let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
        assert_eq!(copied, Some(command));

        // Something else is there: no command makes it go.
        let taken = Cant::Taken("/usr/local/bin/plays".into());
        assert_eq!(taken.command(), None);
        cx.update(|_, cx| tell(Source::Ui, &Err(taken), cx));
        cx.run_until_parked();
        let (_, detail) = cx.pending_prompt().expect("an alert");
        assert!(detail.contains("isn't a link"), "{detail}");
        cx.simulate_prompt_answer("OK");
    }

    #[test]
    fn a_folder_it_cant_write_to_says_what_to_type_instead() {
        let folder = folder("shut");
        fs::set_permissions(&*folder, fs::Permissions::from_mode(0o500)).unwrap();
        let program = Path::new("/Applications/Plays.app/Contents/MacOS/plays");
        let err = link(program, &folder).unwrap_err().to_string();
        let typed = format!(
            "sudo ln -sf '{}' {}",
            program.display(),
            folder.join("plays").display()
        );
        assert!(
            err.contains("ermission denied") && err.ends_with(&typed),
            "{err}"
        );
    }
}
