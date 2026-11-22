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
