//! The command line, against a Plays of the test's own answering on its
//! socket: each command line runs on a thread of its own, as `plays`
//! would in a terminal, or as a process of its own, printing what it
//! prints and exiting as it exits.

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use gpui::{ExternalPaths, FileDropEvent, TestAppContext, VisualTestContext, WindowHandle, point};
use serde_json::{Value, json};

use super::*;
use crate::api::events::cues;
use crate::api::socket::tests::{serving, socket, talk};
use crate::ui::testing::{self, fixture, wait_until};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// What a command line came to: its exit status, and what it printed.
#[derive(Debug)]
pub(super) struct Ran {
    pub(super) status: u8,
    pub(super) out: String,
    pub(super) err: String,
}

impl Ran {
    /// What it printed as JSON.
    pub(super) fn json(&self) -> Value {
        serde_json::from_str(&self.out).unwrap_or_else(|err| panic!("{err}: {self:?}"))
    }

    /// Its status and what it said was wrong.
    pub(super) fn failed(&self) -> (u8, &str) {
        (self.status, self.err.trim_end())
    }
}

/// Run `plays ARGS` with Plays answering at `socket`, if anywhere, doing
/// as `unanswered` says if none does.
pub(super) fn command_line(socket: Option<PathBuf>, unanswered: Unanswered, args: &[&str]) -> Ran {
    command_line_reading(socket, unanswered, args, b"")
}

/// As `command_line`, with `input` on stdin.
fn command_line_reading(
    socket: Option<PathBuf>,
    unanswered: Unanswered,
    args: &[&str],
    mut input: &[u8],
) -> Ran {
    let args = args.iter().map(OsString::from).collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let streams = Streams {
        input: &mut input,
        out: &mut out,
        err: &mut err,
    };
    let status = run(args, socket, unanswered, streams);
    Ran {
        status,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// Run `plays ARGS` against the Plays answering at `path`, on a thread of
/// its own, as the app answers.
pub(super) fn plays(cx: &mut TestAppContext, path: &Path, args: &[&str]) -> Ran {
    plays_reading(cx, path, args, b"")
}

/// As `plays`, with `input` on stdin.
pub(super) fn plays_reading(
    cx: &mut TestAppContext,
    path: &Path,
    args: &[&str],
    input: &[u8],
) -> Ran {
    let (path, input) = (path.to_owned(), input.to_owned());
    let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
    talk(cx, move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        command_line_reading(Some(path), Unanswered::Fails, &args, &input)
    })
}

/// Run `plays ARGS` on a thread of its own, while `meanwhile` runs
/// command lines of its own until it's done, at most a hundred times: as
/// `events` and `wait` hear what those do.
pub(super) fn plays_while(
    cx: &mut TestAppContext,
    path: &Path,
    args: &[&str],
    mut meanwhile: impl FnMut(&mut TestAppContext, usize),
) -> Ran {
    let path_now = path.to_owned();
    let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
    let running = thread::spawn(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        command_line(Some(path_now), Unanswered::Fails, &args)
    });
    for time in 0..100 {
        if running.is_finished() {
            break;
        }
        meanwhile(cx, time);
    }
    assert!(wait_until(cx, |_| running.is_finished()), "it finished");
    running.join().unwrap()
}

pub(super) fn window_id(window: WindowHandle<Workspace>, cx: &TestAppContext) -> String {
    window
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap()
}

fn owned(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[test]
fn subcommands_and_their_options_are_the_command_lines_and_files_plays() {
    for args in [
        &["ls"][..],
        &["window", "ls"],
        &["--json", "ls"],
        &["--json=true"],
        &["-w", "2", "block", "ls"],
        &["-b2"],
        &["--help"],
        &["-h"],
        &["help"],
        &["--version"],
        &["-V"],
        // A subcommand mistyped, not a file.
        &["frob"],
    ] {
        assert!(wanted(&owned(args)), "{args:?} is the command line's");
    }
    for args in [
        &[][..],
        &["film.mkv"],
        &["tests/fixtures"],
        &["/Users/me/Movies/ls"],
        &["https://example.com/live.m3u8"],
        &["-psn_0_12345"],
        // A file by that name.
        &["Cargo.toml"],
        &["src"],
    ] {
        assert!(!wanted(&owned(args)), "{args:?} is Plays'");
    }
}

#[test]
fn help_and_the_version_print_and_a_bad_command_line_says_why() {
    let help = command_line(None, Unanswered::Fails, &["--help"]);
    assert_eq!(help.status, 0);
    assert!(
        help.out.contains("Usage: plays [OPTIONS] <COMMAND>"),
        "{help:?}"
    );
    assert!(help.out.contains("4 no Plays to ask"), "the exit statuses");
    let version = command_line(None, Unanswered::Fails, &["--version"]);
    assert_eq!((version.status, version.out.as_str()), (0, "plays 0.1.0\n"));

    for (args, why) in [
        (&["window", "frob"][..], "unrecognized subcommand 'frob'"),
        (&["frob"], "unrecognized subcommand 'frob'"),
        (&["do"], "<COMMAND|SCRIPT>"),
        (
            &["do", "window.rename", "Den"],
            "arguments are KEY=VALUE, not Den",
        ),
        (
            &["window", "move", "2"],
            "--before <NAME>|--after <NAME>|--index <N>",
        ),
        (&["events", "--all"], "unexpected argument '--all'"),
        (
            &["open", "--new-window", "-w", "1", "a.mkv"],
            "open in a new window, or in -w's",
        ),
        (
            &["api", "call", "app.open", "{urls"],
            "the arguments aren't JSON",
        ),
        (
            &["wait", "playback.ended", "--timeout", "-1"],
            "can't wait -1 seconds",
        ),
        (
            &["do", "--for", "10x", "-e", "return 1"],
            "seconds, as 90 or 90s; minutes, 10m; or hours, 1.5h",
        ),
        (
            &["do", "--for", "1", "window.ls"],
            "--for and --fail-fast are for scripts",
        ),
        (
            &["do", "--fail-fast", "window.ls"],
            "--for and --fail-fast are for scripts",
        ),
    ] {
        let ran = command_line(None, Unanswered::Fails, args);
        assert_eq!(ran.status, 2, "{args:?}: {ran:?}");
        assert!(ran.err.contains(why), "{args:?}: {ran:?}");
        assert_eq!(ran.out, "", "{args:?}");
    }
}

#[test]
fn with_no_plays_to_ask_it_says_so_or_starts_one() {
    let path = socket("cli-none");
    let started = std::cell::Cell::new(0);
    let start: &dyn Fn() -> io::Result<()> = &|| {
        started.set(started.get() + 1);
        Err(io::ErrorKind::NotFound.into())
    };

    let ran = command_line(
        Some(path.clone()),
        Unanswered::Starts(start),
        &["--no-autostart", "ls"],
    );
    assert_eq!(ran.status, 4, "{ran:?}");
    assert!(ran.err.starts_with("plays: can't reach Plays: "), "{ran:?}");
    assert_eq!(started.get(), 0, "not started, as asked");

    let ran = command_line(
        Some(path.clone()),
        Unanswered::Starts(start),
        &["window", "ls"],
    );
    assert_eq!(
        ran.failed(),
        (4, "plays: can't reach Plays: entity not found")
    );
    assert_eq!(started.get(), 1, "started");

    let ran = command_line(None, Unanswered::Starts(start), &["ls"]);
    assert_eq!(ran.status, 4);
    assert!(ran.err.contains("name one with PLAYS_SOCKET"), "{ran:?}");

    let ran = command_line(Some(path), Unanswered::Starts(start), &["version"]);
    assert_eq!(
        (ran.status, ran.out.as_str()),
        (0, "plays 0.1.0\nrunning: none\n")
    );
    assert_eq!(started.get(), 1, "not started just to say");
}

#[test]
fn a_wait_longer_than_anything_lasts_as_long_as_it_takes() {
    let path = socket("cli-forever");
    let args = ["wait", "playback.ended", "--timeout", "1e19"];
    let ran = command_line(Some(path), Unanswered::Fails, &args);
    assert_eq!(ran.status, 4, "on to asking Plays: {ran:?}");
}

#[test]
fn with_control_off_a_plays_that_cant_be_reached_for_another_reason_says_why() {
    let path = socket("cli-off-denied");
    let folder = path.parent().unwrap().to_owned();
    fs::create_dir_all(&folder).unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o000)).unwrap();
    let ran = command_line(Some(path), Unanswered::Off, &["ls"]);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ran.status, 4, "{ran:?}");
    assert!(ran.err.contains("ermission denied"), "{ran:?}");
}

#[gpui::test]
fn with_control_off_here_a_plays_answering_is_asked_still(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("cli-off-answered", cx);
    let ran = talk(&mut cx, move || {
        command_line(Some(path), Unanswered::Off, &["--json", "window", "ls"])
    });
    assert_eq!(ran.status, 0, "{ran:?}");
    assert_eq!(ran.json().as_array().map(Vec::len), Some(1));
}

#[test]
fn actions_lists_every_command_without_plays() {
    let ran = command_line(None, Unanswered::Fails, &["actions"]);
    assert_eq!(ran.status, 0);
    let lines: Vec<&str> = ran.out.lines().collect();
    assert!(
        lines[0].starts_with("ID ") && lines[0].ends_with("KEYS"),
        "{lines:?}"
    );
    assert_eq!(lines.len(), api::all().count() + 1, "a line for each");
    let play = lines
        .iter()
        .find(|line| line.starts_with("playback.play_pause "))
        .unwrap();
    assert!(
        play.contains("Play/Pause") && play.ends_with("Space, P"),
        "{play}"
    );

    let described = command_line(None, Unanswered::Fails, &["--json", "actions"]).json();
    assert_eq!(described.as_array().unwrap().len(), api::all().count());
    assert_eq!(described[0], api::all().next().unwrap().describe());
}

#[test]
fn actions_lists_those_whose_id_or_title_has_what_is_searched_for() {
    let ids = |args: &[&str]| -> Vec<String> {
        let ran = command_line(None, Unanswered::Fails, args);
        assert_eq!(ran.status, 0, "{ran:?}");
        let lines: Vec<&str> = ran.out.lines().collect();
        assert!(lines[0].starts_with("ID "), "{lines:?}");
        lines[1..]
            .iter()
            .map(|line| line.split_whitespace().next().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        ids(&["actions", "video.flip_vertically"]),
        ["video.flip_vertically"],
        "an id"
    );
    let video = ids(&["actions", "video"]);
    let namespace = api::all().filter(|spec| spec.namespace() == "video");
    for spec in namespace {
        assert!(video.contains(&spec.id.to_owned()), "{video:?}");
    }
    let flips = ids(&["actions", "FLIP"]);
    assert!(
        flips.contains(&"video.flip_horizontally".to_owned()) && flips.len() >= 2,
        "a word, in any case: {flips:?}"
    );
    for id in video.iter().chain(&flips) {
        let spec = api::find(id).unwrap();
        let has = |word| {
            format!("{} {}", spec.id, spec.title)
                .to_lowercase()
                .contains(word)
        };
        assert!(has("video") || has("flip"), "{id}");
    }
    assert_eq!(
        ids(&["actions", "play/pause"]),
        ["playback.play_pause", "playback.play_pause_all"],
        "in their titles"
    );

    let described = command_line(None, Unanswered::Fails, &["--json", "actions", "flip"]).json();
    let described: Vec<&str> = described
        .as_array()
        .unwrap()
        .iter()
        .map(|spec| spec["id"].as_str().unwrap())
        .collect();
    assert_eq!(described, flips);

    let ran = command_line(None, Unanswered::Fails, &["actions", "nothing_has_this"]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (3, "", "plays: no command nothing_has_this\n")
    );
    let ran = command_line(None, Unanswered::Fails, &["actions", "video", "--markdown"]);
    assert_eq!(ran.status, 2, "{ran:?}");
}

#[test]
fn completions_are_written_for_each_shell_without_plays() {
    let ran = command_line(None, Unanswered::Fails, &["completions", "zsh"]);
    assert_eq!(ran.status, 0, "{ran:?}");
    assert!(ran.out.starts_with("#compdef plays"), "{}", ran.out);
    let ran = command_line(None, Unanswered::Fails, &["completions", "bash"]);
    assert!(ran.out.contains("complete -F _plays"), "{}", ran.out);
    let ran = command_line(None, Unanswered::Fails, &["completions", "fish"]);
    assert!(ran.out.contains("complete -c plays"), "{}", ran.out);
    let ran = command_line(None, Unanswered::Fails, &["completions", "nushell"]);
    assert!(ran.out.contains("export extern plays ["), "{}", ran.out);
    let ran = command_line(None, Unanswered::Fails, &["completions", "nu"]);
    assert!(ran.out.starts_with("module completions {"), "{}", ran.out);

    let ran = command_line(None, Unanswered::Fails, &["completions", "tcsh"]);
    assert_eq!(ran.status, 2);
    assert!(
        ran.err
            .contains("[possible values: bash, zsh, fish, nushell]"),
        "{}",
        ran.err
    );
    assert!(wanted(&owned(&["completions", "fish"])));
}

#[test]
fn what_it_names_are_windows_and_the_blocks_in_them() {
    // What shows a player in a window is a block: no command, event,
    // argument, namespace or help says otherwise.
    let described = Value::from_iter(api::all().map(api::Spec::describe)).to_string();
    let namespaces: String = api::commands::NAMESPACES
        .iter()
        .map(|(id, title)| format!("{id}: {title}\n"))
        .collect();
    let mut cli = Cli::command();
    cli.build();
    let mut help = String::new();
    every_help(&mut cli, &mut help);
    for (what, text) in [
        ("the reference", reference::reference()),
        ("the commands", described),
        ("the namespaces", namespaces),
        ("the help", help),
    ] {
        assert!(text.contains("block"), "{what} names blocks");
        assert!(!text.to_lowercase().contains("tile"), "{what} names tiles");
    }
}

/// `command`'s help, and each of its subcommands', all the way down.
#[test]
fn a_duration_is_seconds_minutes_or_hours() {
    for (given, seconds) in [
        ("90", 90.0),
        ("90s", 90.0),
        ("0.3s", 0.3),
        ("10m", 600.0),
        ("1.5h", 5400.0),
    ] {
        let lasting = args::lasting(given);
        assert_eq!(
            lasting,
            Ok(std::time::Duration::from_secs_f64(seconds)),
            "{given}"
        );
    }
    for given in ["10x", "-1", "-1m", "m", "", "inf", "NaN"] {
        assert!(args::lasting(given).is_err(), "{given}");
    }
}

fn every_help(command: &mut clap::Command, help: &mut String) {
    help.push_str(&command.render_long_help().to_string());
    for sub in command.get_subcommands_mut() {
        every_help(sub, help);
    }
}

#[test]
#[cfg(target_os = "macos")]
fn the_command_reference_is_current() {
    let kept = include_str!("../../docs/commands.md");
    assert!(
        reference::reference() == kept,
        "docs/commands.md isn't as the commands are: \
         cargo run -- actions --markdown > docs/commands.md"
    );
    let ran = command_line(None, Unanswered::Fails, &["actions", "--markdown"]);
    assert_eq!(ran.out, reference::reference());
    #[cfg(debug_assertions)]
    assert!(
        api::find("app.show_components").is_some() && !kept.contains("app.show_components"),
        "but not the commands for working on Plays, which release builds don't have"
    );
    let play = "### Play/Pause\n\n`playback.play_pause`: ";
    assert!(kept.contains(play), "each command under its title");
    assert!(kept.contains("- Keys: Space, P\n"));
    assert!(
        kept.contains("  - `time` (a number, at least 0): "),
        "arguments"
    );
}

#[gpui::test]
fn with_control_off_it_says_where_to_turn_it_on(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("cli-off", cx);
    crate::api::socket::tests::allow_control(&mut cx, false);
    for args in [
        &["ls"][..],
        &["do", "playback.loop.toggle"],
        &["do", "-e", "return 1"],
        &["events"],
    ] {
        let asking = path.clone();
        let ran = talk(&mut cx, move || {
            command_line(Some(asking), Unanswered::Off, args)
        });
        assert_eq!(
            ran.failed(),
            (
                5,
                "plays: control from the command line is off: turn it on in Settings → \
                 Command Line"
            ),
            "{args:?}"
        );
    }
    let ran = talk(&mut cx, move || {
        command_line(Some(path), Unanswered::Off, &["version"])
    });
    assert_eq!(
        (ran.status, ran.out.as_str()),
        (0, "plays 0.1.0\nrunning: none\n"),
        "who runs is no control"
    );
}

#[gpui::test]
fn a_command_runs_with_its_arguments_and_says_what_it_handed_back(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-do", cx);
    let ran = plays(&mut cx, &path, &["do", "window.rename", "label=1984"]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (0, "", "")
    );
    let label = window.read_with(&cx, |workspace, _| workspace.label().map(str::to_owned));
    assert_eq!(
        label.unwrap().as_deref(),
        Some("1984"),
        "text, as the argument takes"
    );

    let ran = plays(&mut cx, &path, &["do", "app.settings.get", "key=autoplay"]);
    assert_eq!((ran.status, ran.out.as_str()), (0, "true\n"));
    let ran = plays(&mut cx, &path, &["--json", "do", "window.new"]);
    assert_eq!(
        ran.json()["window"].as_str().map(str::len),
        Some(8),
        "{ran:?}"
    );

    let ran = plays(&mut cx, &path, &["do", "playback.nope"]);
    assert_eq!(ran.failed(), (1, "plays: no command playback.nope"));
    let ran = plays(&mut cx, &path, &["do", "window.move", "position=first"]);
    assert_eq!(ran.status, 2, "{ran:?}");
    let ran = plays(&mut cx, &path, &["-w", "Kitchen", "do", "window.focus"]);
    assert_eq!(ran.status, 3, "{ran:?}");
    assert!(ran.err.contains("Kitchen"), "{ran:?}");
}

#[test]
fn arguments_are_typed_as_their_commands_take_them() {
    let args = |command: &str, pairs: &[&str]| {
        let pairs: Vec<String> = pairs.iter().map(|&pair| pair.to_owned()).collect();
        arguments(command, &pairs).unwrap()
    };
    assert_eq!(args("window.rename", &[]), Value::Null);
    assert_eq!(
        args("window.rename", &["label=1984"]),
        json!({"label": "1984"})
    );
    assert_eq!(args("playback.seek", &["time=90.5"]), json!({"time": 90.5}));
    assert_eq!(
        args("app.open", &["urls=a.mkv"]),
        json!({"urls": ["a.mkv"]}),
        "a list"
    );
    assert_eq!(
        args("app.open", &["urls=a.mkv", "urls=b.mkv"]),
        json!({"urls": ["a.mkv", "b.mkv"]})
    );
    assert_eq!(
        args("app.open", &["urls=a.mkv", "autoplay=false"]),
        json!({"urls": ["a.mkv"], "autoplay": false}),
        "true or false"
    );
    assert_eq!(
        args("app.settings.set", &["key=subtitles.size", "value=30"]),
        json!({"key": "subtitles.size", "value": 30}),
        "any value read as JSON"
    );
    assert_eq!(
        args("app.settings.set", &["key=theme", "value=dark"]),
        json!({"key": "theme", "value": "dark"}),
        "as given, if it isn't"
    );
    assert_eq!(
        args(
            "window.set_ui",
            &["visible=controls,notes", "visible=top_bar"]
        ),
        json!({"visible": ["controls", "notes", "top_bar"]}),
        "a list of words, apart at commas"
    );
    assert_eq!(
        args("window.set_ui", &["visible="]),
        json!({"visible": []}),
        "none"
    );
    assert_eq!(
        args("no.such", &["a=1", "a=x"]),
        json!({"a": [1, "x"]}),
        "again, a list"
    );
}

#[gpui::test]
fn windows_and_their_blocks_are_listed(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-ls", cx);
    let ran = plays(
        &mut cx,
        &path,
        &["open", "tests/fixtures/clip.mp4", "tests/fixtures/tone.m4a"],
    );
    assert_eq!((ran.status, ran.out.as_str()), (0, ""), "{ran:?}");
    let opened = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, _| workspace.players().len())
            .unwrap()
            == 2
    });
    assert!(opened);
    let id = window_id(window, &cx);

    let ran = plays(&mut cx, &path, &["ls"]);
    let lines: Vec<&str> = ran.out.lines().collect();
    assert_eq!(lines[0], format!("* 1  {id}  -"), "{lines:?}");
    assert!(
        lines[1].starts_with("    * 1  ") && lines[1].contains("clip.mp4"),
        "{lines:?}"
    );
    assert!(
        lines[2].starts_with("      2  ") && lines[2].contains("tone.m4a"),
        "{lines:?}"
    );
    assert_eq!(lines.len(), 3);

    let listed = plays(&mut cx, &path, &["--json", "ls"]).json();
    assert_eq!(listed[0]["id"], id.as_str());
    assert_eq!(
        listed[0]["blocks"][1]["title"], "tone.m4a",
        "the blocks, not how many"
    );

    let ran = plays(&mut cx, &path, &["window", "ls"]);
    assert_eq!(
        ran.out,
        format!(
            "   #  ID{}  LABEL  BLOCKS\n*  1  {id}  -      2\n",
            " ".repeat(6)
        )
    );
    let ran = plays(&mut cx, &path, &["block", "ls"]);
    let lines: Vec<&str> = ran.out.lines().collect();
    assert!(
        lines[0].starts_with("   #  ID") && lines[0].ends_with("NAME      STATE"),
        "{lines:?}"
    );
    assert!(
        lines[2].contains("tone.m4a  paused") || lines[2].contains("tone.m4a  playing"),
        "{lines:?}"
    );
    let blocks = plays(&mut cx, &path, &["--json", "-w", "1", "block", "ls"]).json();
    assert_eq!(blocks.as_array().unwrap().len(), 2);
}

#[gpui::test]
fn windows_are_opened_named_moved_and_closed(cx: &mut TestAppContext) {
    let (path, first, mut cx) = serving("cli-windows", cx);
    let first_id = window_id(first, &cx);
    let ran = plays(&mut cx, &path, &["window", "new", "Kitchen"]);
    assert_eq!(ran.status, 0, "{ran:?}");
    let kitchen = ran.out.trim_end().to_owned();
    assert_eq!(kitchen.len(), 8, "its id: {ran:?}");
    plays(&mut cx, &path, &["window", "new"]);
    let numbers = |cx: &mut TestAppContext| -> Vec<(String, Value)> {
        let listed = plays(cx, &path, &["--json", "window", "ls"]).json();
        listed
            .as_array()
            .unwrap()
            .iter()
            .map(|window| (show::cell(&window["label"]), window["number"].clone()))
            .collect()
    };
    assert_eq!(
        numbers(&mut cx),
        [
            ("-".into(), json!(1)),
            ("Kitchen".into(), json!(2)),
            ("-".into(), json!(3))
        ]
    );

    plays(&mut cx, &path, &["window", "rename", "Den"]);
    plays(&mut cx, &path, &["window", "rename", "Kitchen", "Galley"]);
    plays(&mut cx, &path, &["window", "rename", "1", "Hall"]);
    let labels: Vec<String> = numbers(&mut cx)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(
        labels,
        ["Hall", "Galley", "Den"],
        "the one in front, then those named"
    );

    plays(
        &mut cx,
        &path,
        &["window", "move", "Hall", "--after", "Den"],
    );
    let labels: Vec<String> = numbers(&mut cx)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(labels, ["Galley", "Den", "Hall"]);
    plays(
        &mut cx,
        &path,
        &["window", "move", "Hall", "--before", "Den"],
    );
    plays(
        &mut cx,
        &path,
        &["-w", "Galley", "window", "move", "--index", "3"],
    );
    let labels: Vec<String> = numbers(&mut cx)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(labels, ["Hall", "Den", "Galley"]);
    let ran = plays(&mut cx, &path, &["window", "move", "Den", "--after", "Den"]);
    assert_eq!(
        ran.failed(),
        (2, "plays: a window can't move beside itself")
    );

    plays(&mut cx, &path, &["window", "focus", "Galley"]);
    let inspected = plays(&mut cx, &path, &["--json", "window", "inspect"]).json();
    assert_eq!(inspected["label"], "Galley", "in front");
    plays(&mut cx, &path, &["window", "focus", "next"]);
    let inspected = plays(&mut cx, &path, &["--json", "window", "inspect"]).json();
    assert_eq!(inspected["label"], "Hall", "the next, round to the first");

    // 1 and 2 as they are before either closes.
    let ran = plays(&mut cx, &path, &["window", "close", "1", "2"]);
    assert_eq!(ran.status, 0, "{ran:?}");
    let labels: Vec<String> = numbers(&mut cx)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert_eq!(labels, ["Galley"]);
    assert_ne!(kitchen, first_id);
    let ran = plays(&mut cx, &path, &["window", "inspect", "Hall"]);
    assert_eq!(ran.status, 3, "{ran:?}");
}

#[gpui::test]
fn blocks_are_focused_maximized_renamed_moved_and_closed(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-blocks", cx);
    plays(
        &mut cx,
        &path,
        &[
            "open",
            "tests/fixtures/clip.mp4",
            "tests/fixtures/tone.m4a",
            "tests/fixtures/clip-ip.mkv",
        ],
    );
    let opened = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, _| workspace.players().len())
            .unwrap()
            == 3
    });
    assert!(opened);
    let names = |cx: &mut TestAppContext| -> Vec<String> {
        let blocks = plays(cx, &path, &["--json", "block", "ls"]).json();
        listed(&blocks)
            .map(|block| match &block["label"] {
                Value::Null => show::text(&block["title"]),
                label => show::text(label),
            })
            .collect()
    };
    plays(&mut cx, &path, &["block", "rename", "1", "Clip"]);
    plays(&mut cx, &path, &["block", "rename", "2", "Tone"]);
    assert_eq!(names(&mut cx), ["Clip", "Tone", "clip-ip.mkv"]);

    plays(&mut cx, &path, &["block", "focus", "Tone"]);
    let inspected = plays(&mut cx, &path, &["--json", "block", "inspect"]).json();
    assert_eq!(inspected["label"], "Tone", "keys go to it");
    plays(&mut cx, &path, &["block", "rename", "Sound"]);
    assert_eq!(
        names(&mut cx),
        ["Clip", "Sound", "clip-ip.mkv"],
        "the one keys go to"
    );

    plays(&mut cx, &path, &["block", "move", "Clip", "--after", "3"]);
    assert_eq!(names(&mut cx), ["Sound", "clip-ip.mkv", "Clip"]);
    plays(
        &mut cx,
        &path,
        &["-b", "Clip", "block", "move", "--index", "1"],
    );
    assert_eq!(names(&mut cx), ["Clip", "Sound", "clip-ip.mkv"]);

    let ran = plays(&mut cx, &path, &["block", "zoom", "Sound"]);
    assert_eq!(ran.status, 0, "{ran:?}");
    let zoomed = window
        .update(&mut cx, |workspace, window, cx| {
            workspace.seen(window, cx).maximized
        })
        .unwrap();
    assert!(zoomed.is_some(), "it fills the window");
    plays(&mut cx, &path, &["block", "zoom", "Sound"]);

    let ran = plays(&mut cx, &path, &["block", "close", "1", "2"]);
    assert_eq!(ran.status, 0, "{ran:?}");
    assert_eq!(names(&mut cx), ["clip-ip.mkv"], "1 and 2 as they were");
    let ran = plays(&mut cx, &path, &["block", "inspect", "Clip"]);
    assert_eq!(ran.status, 3, "{ran:?}");
}

#[gpui::test]
fn files_open_by_their_full_paths_in_a_new_window_or_the_one_named(cx: &mut TestAppContext) {
    let (path, first, mut cx) = serving("cli-open", cx);
    let ran = plays(
        &mut cx,
        &path,
        &["--json", "open", "--new-window", "tests/fixtures/clip.mp4"],
    );
    assert_eq!(ran.status, 0, "{ran:?}");
    let opened_in = ran.json()["window"].as_str().unwrap().to_owned();
    assert_ne!(opened_in, window_id(first, &cx), "a new window");
    let clip = fixture("clip.mp4").display().to_string();
    let shown = wait_until(&mut cx, |cx| {
        let blocks = plays(cx, &path, &["--json", "-w", "2", "block", "ls"]).json();
        blocks[0]["file"] == clip.as_str()
    });
    assert!(shown, "by its full path");

    plays(
        &mut cx,
        &path,
        &["-w", "1", "open", "tests/fixtures/tone.m4a"],
    );
    let shown = wait_until(&mut cx, |cx| {
        first
            .read_with(cx, |workspace, _| workspace.players().len())
            .unwrap()
            == 1
    });
    assert!(shown, "in the window named");
    let ran = plays(&mut cx, &path, &["open", "gopher://example.com/film"]);
    assert_eq!(ran.status, 2, "{ran:?}");

    // A block moves within its grid: to another window by a command of its
    // own.
    let id = |cx: &mut TestAppContext, window| {
        let block = plays(cx, &path, &["--json", "-w", window, "block", "inspect"]).json();
        show::text(&block["id"])
    };
    let (near, far) = (id(&mut cx, "1"), id(&mut cx, "2"));
    let ran = plays(&mut cx, &path, &["block", "move", &near, "--after", &far]);
    assert_eq!(ran.status, 2, "{ran:?}");
    assert!(
        ran.err
            .contains("is in another window: move blocks between windows"),
        "{ran:?}"
    );
}

#[gpui::test]
fn events_print_as_they_happen(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("cli-events", cx);
    // Each command line connecting is heard.
    let ran = plays_while(&mut cx, &path, &["events", "--count", "2"], |cx, _| {
        plays(cx, &path, &["version"]);
    });
    assert_eq!(ran.status, 0, "{ran:?}");
    let lines: Vec<&str> = ran.out.lines().collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines.iter().all(|line| line.starts_with("app.client_")),
        "{lines:?}"
    );

    let ran = plays_while(
        &mut cx,
        &path,
        &["--json", "-w", "1", "events", "--count", "1"],
        |cx, time| {
            let label = if time % 2 == 0 {
                "label=Den"
            } else {
                "label=Hall"
            };
            plays(cx, &path, &["-w", "1", "do", "window.rename", label]);
        },
    );
    let event: Value = serde_json::from_str(ran.out.trim_end()).unwrap();
    assert_eq!(event["type"], "window.label_changed", "{ran:?}");
    assert!(event["payload"]["window_id"].is_string());
}

/// Plain `plays events` hears everything, each window's blocks too: a file
/// opened that starts playing, where it is said, its volume however it's
/// changed, and its pausing, each after its command.
#[gpui::test]
fn events_hear_every_window_and_block_unless_one_is_named(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-events-all", cx);
    let playing = move |cx: &mut TestAppContext, playing: bool| {
        let now = wait_until(cx, |cx| {
            playing == testing::all_started(window, 1, cx)
                && window
                    .read_with(cx, |workspace, cx| {
                        workspace.players().first().is_some_and(|view| {
                            let view = view.read(cx);
                            view.player().is_known() && view.is_playing() == playing
                        })
                    })
                    .unwrap()
        });
        assert!(now, "playing: {playing}");
        // What changed is told now, as the app does every `LOOK_EVERY`.
        cx.update(api::events::look);
    };
    let ran = plays_while(
        &mut cx,
        &path,
        &["--json", "events", "--count", "60"],
        |cx, time| {
            if time > 0 {
                // Clients come and go till it has heard enough.
                plays(cx, &path, &["version"]);
                return;
            }
            let listens = wait_until(cx, |cx| cx.update(api::events::listeners) == 1);
            assert!(listens, "it listens");
            plays(cx, &path, &["open", "tests/fixtures/clip.mp4"]);
            playing(cx, true);
            for command in [
                &["do", "audio.volume_down"][..],
                &["do", "audio.volume_up"],
                &["do", "audio.set_volume", "volume=30"],
            ] {
                assert_eq!(plays(cx, &path, command).status, 0, "{command:?}");
                cx.update(api::events::look);
            }
            plays(cx, &path, &["do", "playback.play_pause"]);
            playing(cx, false);
        },
    );
    assert_eq!(ran.status, 0, "{ran:?}");
    let heard: Vec<Value> = ran
        .out
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let said: Vec<&str> = heard
        .iter()
        .filter_map(|event| match event["type"].as_str()? {
            "app.command_run" => event["payload"]["command"]
                .as_str()
                .filter(|command| !command.starts_with("app.")),
            kind @ ("playback.opened"
            | "playback.playing"
            | "audio.volume_changed"
            | "playback.paused") => Some(kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        said,
        [
            "playback.opened",
            "playback.playing",
            "audio.volume_down",
            "audio.volume_changed",
            "audio.volume_up",
            "audio.volume_changed",
            "audio.set_volume",
            "audio.volume_changed",
            "playback.play_pause",
            "playback.paused",
        ],
        "{heard:#?}"
    );
    let told = |kind: &'static str| {
        heard
            .iter()
            .filter(move |event| event["type"] == kind)
            .map(|event| &event["payload"])
    };
    let opened = told("playback.opened").next().unwrap();
    assert_eq!(opened["block_title"], "clip.mp4", "{opened}");
    assert_eq!(opened["window_title"], "Plays", "{opened}");
    for payload in told("playback.playing").chain(told("playback.paused")) {
        assert!(payload["position"].is_number(), "{payload}");
        assert!(payload["time"].is_number(), "{payload}");
        assert!(payload["frame"].is_u64(), "{payload}");
    }
    let volumes: Vec<&Value> = told("audio.volume_changed")
        .map(|payload| &payload["volume"])
        .collect();
    assert!(
        volumes[0].as_f64() < volumes[1].as_f64(),
        "down, then up: {volumes:?}"
    );
    assert_eq!(volumes[2], &json!(30.0));
}

/// Drop `files` on the middle of `window` as the system does: dragged in,
/// then let go.
fn drop_files(window: WindowHandle<Workspace>, cx: &TestAppContext, files: &[PathBuf]) {
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    let position = point(WINDOW_SIZE.width / 2., WINDOW_SIZE.height / 2.);
    let paths = ExternalPaths::for_test(files.iter().cloned());
    cx.simulate_event(FileDropEvent::Entered { position, paths });
    cx.simulate_event(FileDropEvent::Submit { position });
    cx.run_until_parked();
}

/// Files dropped on a window open with no command run, each playing as it
/// opens: plain `plays events` hears each block opened, then playing, by
/// its own id and title.
#[gpui::test]
fn files_dropped_are_heard_each_opened_then_playing(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-events-dropped", cx);
    let files = ["clip.mp4", "clip-ip.mkv", "clip-chapters.mkv"].map(fixture);
    let ran = plays_while(
        &mut cx,
        &path,
        &["--json", "events", "--count", "40"],
        |cx, time| {
            if time > 0 {
                // Clients come and go till it has heard enough.
                plays(cx, &path, &["version"]);
                return;
            }
            let listens = wait_until(cx, |cx| cx.update(api::events::listeners) == 1);
            assert!(listens, "it listens");
            drop_files(window, cx, &files);
            let playing = wait_until(cx, |cx| testing::all_started(window, files.len(), cx));
            assert!(playing, "each file dropped plays");
            // What changed is told now, as the app does every `LOOK_EVERY`.
            cx.update(api::events::look);
        },
    );
    assert_eq!(ran.status, 0, "{ran:?}");
    let heard: Vec<Value> = ran
        .out
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(
        !heard.iter().any(|event| event["type"] == "app.command_run"
            && event["payload"]["command"]
                .as_str()
                .is_some_and(|command| command.contains("open"))),
        "a drop runs no command: {heard:#?}"
    );
    let window_id = window_id(window, &cx);
    let blocks: Vec<(String, String)> = window
        .read_with(&cx, |workspace, cx| {
            workspace
                .players()
                .iter()
                .map(|view| {
                    let view = view.read(cx);
                    (view.id().to_string(), view.header_title().to_string())
                })
                .collect()
        })
        .unwrap();
    assert_eq!(blocks.len(), files.len());
    for (block, title) in &blocks {
        let told = |kind: &str| {
            heard.iter().position(|event| {
                event["type"] == kind && event["payload"]["block_id"] == block.as_str()
            })
        };
        let opened =
            told("playback.opened").unwrap_or_else(|| panic!("{title} opened: {heard:#?}"));
        let playing =
            told("playback.playing").unwrap_or_else(|| panic!("{title} playing: {heard:#?}"));
        assert!(opened < playing, "{title}: opened, then playing");
        for at in [opened, playing] {
            let payload = &heard[at]["payload"];
            assert_eq!(payload["block_title"], title.as_str(), "{payload}");
            assert_eq!(payload["window_id"], window_id.as_str(), "{payload}");
        }
        let playing = &heard[playing]["payload"];
        assert!(playing["position"].is_number(), "{playing}");
        assert!(playing["frame"].is_u64(), "{playing}");
    }
}

/// Wait until `plays wait`, on another thread, has looked at the block it
/// waits for, hearing already: what happens to it from then on is heard.
fn looked(cx: &mut TestAppContext) {
    let inspected = wait_until(cx, |cx| {
        let ran = cx.update(api::recorded);
        ran.iter()
            .any(|invocation| invocation.command == "block.inspect")
    });
    assert!(inspected, "it looked at the block");
}

#[gpui::test]
fn waiting_for_a_block_ends_with_its_event_or_says_why_not(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-wait", cx);
    plays(
        &mut cx,
        &path,
        &["open", "tests/fixtures/clip.mp4", "tests/fixtures/tone.m4a"],
    );
    let opened = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, _| workspace.players().len())
            .unwrap()
            == 2
    });
    assert!(opened);

    TestAppContext::update(&cx, api::record);
    let ran = plays_while(
        &mut cx,
        &path,
        &["wait", "block.label_changed", "-b", "2"],
        |cx, time| {
            if time == 0 {
                looked(cx);
                plays(cx, &path, &["-b", "2", "do", "block.rename", "label=Low"]);
            }
        },
    );
    assert_eq!(ran.status, 0, "{ran:?}");
    assert!(
        ran.out.starts_with("block.label_changed window_id="),
        "{ran:?}"
    );
    assert_eq!(ran.out.lines().count(), 1);

    let ran = plays(
        &mut cx,
        &path,
        &["wait", "playback.playing", "-b", "1", "--timeout", "0.2"],
    );
    assert_eq!(
        ran.failed(),
        (1, "plays: no playback.playing in 0.2 seconds")
    );
    let ran = plays(&mut cx, &path, &["wait", "playback.ended", "-b", "Attic"]);
    assert_eq!(ran.status, 3, "{ran:?}");

    // Where a block is, told only to those who ask, as waiting for it does.
    TestAppContext::update(&cx, api::record);
    let ran = plays_while(
        &mut cx,
        &path,
        &["wait", "playback.position", "-b", "1", "--timeout", "5"],
        |cx, time| {
            if time == 0 {
                looked(cx);
                plays(cx, &path, &["-b", "1", "do", "playback.play_pause"]);
            }
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(100));
            cx.run_until_parked();
            thread::sleep(std::time::Duration::from_millis(20));
        },
    );
    assert_eq!(ran.status, 0, "{ran:?}");
    assert!(
        ran.out.starts_with("playback.position window_id="),
        "{ran:?}"
    );

    TestAppContext::update(&cx, api::record);
    let ran = plays_while(
        &mut cx,
        &path,
        &["wait", "playback.ended", "-b", "1"],
        |cx, time| {
            if time == 0 {
                // Closed first, the other would be block 1.
                looked(cx);
                plays(cx, &path, &["block", "close", "1"]);
            }
        },
    );
    assert_eq!(
        ran.failed(),
        (1, "plays: the block closed before its playback.ended")
    );
}

/// Open the clip in `window`, and wait until it plays.
pub(super) fn playing_the_clip(
    cx: &mut TestAppContext,
    path: &Path,
    window: WindowHandle<Workspace>,
) {
    plays(cx, path, &["open", "tests/fixtures/clip.mp4"]);
    let playing = wait_until(cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                let players = workspace.players();
                let first = players.first();
                first.is_some_and(|block| block.read(cx).player().is_playing())
            })
            .unwrap()
    });
    assert!(playing, "the clip plays");
}

/// The app's clock moving on a step, as a block plays.
pub(super) fn step(cx: &mut TestAppContext) {
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();
    thread::sleep(std::time::Duration::from_millis(20));
}

#[gpui::test]
fn a_cue_is_heard_as_its_block_reaches_it_and_waited_for(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-cue", cx);
    playing_the_clip(&mut cx, &path, window);
    let id = window
        .read_with(&cx, |workspace, cx| workspace.players()[0].read(cx).id())
        .unwrap();
    let block = id.to_string();
    let cue = format!("block.{block}.position@1s");
    let from_the_start = ["-b", &block, "do", "playback.seek", "time=0"];

    // Among a window's events, though the block is another's: a window of
    // its own, where nothing else happens.
    plays(&mut cx, &path, &["window", "new"]);
    plays(&mut cx, &path, &from_the_start);
    let ran = plays_while(
        &mut cx,
        &path,
        &["--json", "-w", "2", "events", "--cue", &cue, "--count", "1"],
        |cx, _| step(cx),
    );
    assert_eq!(ran.status, 0, "{ran:?}");
    let heard = ran.json();
    assert_eq!(heard["type"], json!(cue), "{ran:?}");
    assert_eq!(heard["payload"]["block_id"], json!(block));
    assert!(heard["payload"]["time"].as_f64().unwrap() >= 1.0, "{ran:?}");

    plays(&mut cx, &path, &from_the_start);
    let ran = plays_while(
        &mut cx,
        &path,
        &["wait", &cue, "--timeout", "10"],
        |cx, _| step(cx),
    );
    assert_eq!(ran.status, 0, "{ran:?}");
    assert!(ran.out.starts_with(&format!("{cue} window_id=")), "{ran:?}");
    assert_eq!(ran.out.lines().count(), 1);

    let ran = plays(&mut cx, &path, &["wait", "block.Attic.position@1s"]);
    assert_eq!(ran.failed(), (3, "plays: no block Attic"));
    let ran = plays(
        &mut cx,
        &path,
        &["wait", &format!("block.{block}.position@1")],
    );
    assert_eq!(ran.status, 2, "{ran:?}");

    let late = format!("block.{block}.position@60s");
    let ran = plays_while(&mut cx, &path, &["wait", &late], |cx, time| {
        if time == 0 {
            let set = wait_until(cx, |cx| cx.update(|cx| cues::watched(id, cx)));
            assert!(set, "it set its cue");
            plays(cx, &path, &["block", "close", &block]);
        }
    });
    let closed = format!("plays: the block closed before its {late}");
    assert_eq!(ran.failed(), (1, closed.as_str()));
}

/// As Plays quits, `events` prints each window closing, then `app.quit`, and
/// ends; `wait` says Plays quit before what it waits for, or that what it
/// watches closed first, unless it waits for `app.quit`.
#[gpui::test]
fn events_end_and_waits_fail_as_plays_quits(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-quit", cx);
    let w = window_id(window, &cx);
    TestAppContext::update(&cx, api::record);
    let lines: [&[&'static str]; 5] = [
        &["--json", "events"],
        &["-w", "1", "events"],
        &["wait", "playback.ended"],
        &["wait", "app.quit"],
        &["-w", "1", "wait", "playback.ended"],
    ];
    let running: Vec<thread::JoinHandle<Ran>> = lines
        .iter()
        .map(|args| {
            let (path, args) = (path.clone(), args.to_vec());
            thread::spawn(move || command_line(Some(path), Unanswered::Fails, &args))
        })
        .collect();
    let listening = wait_until(&mut cx, |cx| {
        let inspected = cx
            .update(api::recorded)
            .iter()
            .any(|invocation| invocation.command == "window.inspect");
        inspected && cx.update(api::events::listeners) == lines.len()
    });
    assert!(listening, "each listens");

    cx.quit();
    let ended = wait_until(&mut cx, |_| {
        running.iter().all(thread::JoinHandle::is_finished)
    });
    assert!(ended, "each ended");
    let [all, first, ended, quit, closed] = running
        .into_iter()
        .map(|running| running.join().unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    assert_eq!(all.status, 0, "{all:?}");
    let kinds: Vec<Value> = all
        .out
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["type"].clone())
        .collect();
    assert_eq!(
        kinds[kinds.len() - 3..],
        ["window.closed", "window.active_changed", "app.quit"],
        "{all:?}"
    );
    assert_eq!(
        (first.status, first.out.as_str()),
        (
            0,
            format!("window.closed window_id={w} window_title=Plays\napp.quit\n").as_str()
        ),
        "{first:?}"
    );
    assert_eq!(
        ended.failed(),
        (1, "plays: Plays quit before its playback.ended")
    );
    assert_eq!(
        (quit.status, quit.out.as_str()),
        (0, "app.quit\n"),
        "{quit:?}"
    );
    assert_eq!(
        closed.failed(),
        (1, "plays: the window closed before its playback.ended")
    );
}

#[gpui::test]
fn settings_are_listed_read_and_changed(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("cli-settings", cx);
    let ran = plays(&mut cx, &path, &["settings", "set", "autoplay", "false"]);
    assert_eq!((ran.status, ran.out.as_str()), (0, "false\n"), "{ran:?}");
    let ran = plays(&mut cx, &path, &["settings", "get", "autoplay"]);
    assert_eq!(ran.out, "false\n");
    let ran = plays(
        &mut cx,
        &path,
        &["--json", "settings", "set", "theme", "dark"],
    );
    assert_eq!(ran.out, "\"dark\"\n");
    let ran = plays(&mut cx, &path, &["settings", "ls"]);
    let lines: Vec<&str> = ran.out.lines().collect();
    assert!(
        lines[0].starts_with("KEY ") && lines[0].ends_with("VALUE"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("autoplay ") && line.ends_with(" false"))
    );
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("theme ") && line.ends_with(" dark"))
    );
    let ran = plays(&mut cx, &path, &["settings", "get", "nope"]);
    assert_eq!(ran.failed(), (3, "plays: no setting nope"));
}

#[gpui::test]
fn the_keymap_lists_the_keys_commands_have_now(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("cli-keymap", cx);
    let keys = r#"{"playback.play_pause": ["cmd-p"]}"#;
    let path_now = path.clone();
    let ran = talk(&mut cx, move || {
        command_line(
            Some(path_now),
            Unanswered::Fails,
            &["settings", "set", "hotkeys", keys],
        )
    });
    assert_eq!(ran.status, 0, "{ran:?}");
    let ran = plays(&mut cx, &path, &["keymap"]);
    let lines: Vec<&str> = ran.out.lines().collect();
    assert!(
        lines[0].starts_with("KEYS ") && lines[0].ends_with("TITLE"),
        "{lines:?}"
    );
    let play = lines
        .iter()
        .find(|line| line.contains(" playback.play_pause "))
        .unwrap();
    assert!(
        play.starts_with("⌘P ") && play.ends_with("Play/Pause"),
        "{play}"
    );
    let keymap = plays(&mut cx, &path, &["--json", "keymap"]).json();
    let play = listed(&keymap).find(|command| command["command"] == "playback.play_pause");
    assert_eq!(play.unwrap()["keys"], json!(["cmd-p"]));
}

#[gpui::test]
fn the_api_is_listed_described_and_called(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-api", cx);
    let ran = plays(&mut cx, &path, &["api", "list"]);
    assert_eq!(ran.out.lines().count(), api::all().count() + 1, "{ran:?}");
    let listed = plays(&mut cx, &path, &["--json", "api", "list"]).json();
    assert_eq!(listed[0], api::all().next().unwrap().describe());
    let described = plays(&mut cx, &path, &["api", "describe", "block.focus"]).json();
    assert_eq!(described["title"], "Go to Block");
    let ran = plays(&mut cx, &path, &["api", "describe", "block.nope"]);
    assert_eq!(ran.status, 1);

    let ran = plays(
        &mut cx,
        &path,
        &["api", "call", "window.rename", r#"{"label": "Den"}"#],
    );
    assert_eq!((ran.status, ran.out.as_str()), (0, "null\n"), "{ran:?}");
    let label = window.read_with(&cx, |workspace, _| workspace.label().map(str::to_owned));
    assert_eq!(label.unwrap().as_deref(), Some("Den"));
}

#[gpui::test]
fn the_version_says_which_plays_runs(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("cli-version", cx);
    let ran = plays(&mut cx, &path, &["version"]);
    let pid = std::process::id();
    assert_eq!(
        ran.out,
        format!("plays 0.1.0\nrunning: Plays 0.1.0, process {pid}\n")
    );
    let ran = plays(&mut cx, &path, &["--json", "version"]);
    assert_eq!(
        ran.json(),
        json!({"version": "0.1.0", "running": {"version": "0.1.0", "pid": pid}})
    );
}

/// Output that can't be written: closed, or full.
struct Closed(io::ErrorKind);

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(self.0.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn output_closed_ends_it_quietly_and_other_trouble_says_so() {
    let ran = |kind| {
        let mut err = Vec::new();
        let args = owned(&["actions"]);
        let streams = Streams {
            input: &mut io::empty(),
            out: &mut Closed(kind),
            err: &mut err,
        };
        let status = run(args, None, Unanswered::Fails, streams);
        (status, String::from_utf8(err).unwrap())
    };
    assert_eq!(
        ran(io::ErrorKind::BrokenPipe),
        (0, String::new()),
        "as | head closes it"
    );
    let (status, said) = ran(io::ErrorKind::StorageFull);
    assert_eq!(status, 1);
    assert!(said.starts_with("plays: can't print: "), "{said}");
}

/// Started as another process by the test after it, as `plays ARGS` would
/// be in a terminal: runs the command line `PLAYS_CLI` lists, a line
/// each, against the Plays answering at `PLAYS_SOCKET`, and exits with its
/// status, as `main` does. Passes by itself.
#[test]
#[ignore = "started as another process by the test after it"]
fn command_line_as_another_process() {
    let Some(args) = std::env::var("PLAYS_CLI").ok() else {
        return;
    };
    let args: Vec<OsString> = args.lines().map(OsString::from).collect();
    io::stdout().flush().ok();
    std::process::exit(main(args).into());
}

/// `plays ARGS` started as a process of its own: this test binary,
/// running only `command_line_as_another_process`.
fn start_another_process(path: &Path, args: &[&str]) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "--quiet",
            "cli::tests::command_line_as_another_process",
        ])
        .env("PLAYS_SOCKET", path)
        .env("PLAYS_CLI", args.join("\n"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// How `plays ARGS` exits, and what it prints, run as a process of its
/// own: this test binary, running only `command_line_as_another_process`.
fn another_process(
    cx: &mut TestAppContext,
    path: &Path,
    args: &[&str],
) -> (Option<i32>, String, String) {
    let mut plays = start_another_process(path, args);
    let mut exited = None;
    assert!(wait_until(cx, |_| {
        exited = plays.try_wait().unwrap();
        exited.is_some()
    }));
    let output = plays.wait_with_output().unwrap();
    // After what the test harness says as it starts.
    let out = String::from_utf8(output.stdout).unwrap();
    let out = out
        .split_once("running 1 test\n")
        .map_or(out.as_str(), |(_, out)| out)
        .to_owned();
    (
        exited.unwrap().code(),
        out,
        String::from_utf8(output.stderr).unwrap(),
    )
}

#[gpui::test]
fn ctrl_c_stops_a_script_where_it_is_and_runs_its_on_stop_function(cx: &mut TestAppContext) {
    use std::os::unix::process::ExitStatusExt;

    let (path, window, mut cx) = serving("cli-ctrl-c", cx);
    let label = |cx: &TestAppContext| {
        window
            .read_with(cx, |workspace, _| workspace.label().map(str::to_owned))
            .unwrap()
    };
    // "Went on" if the script went on past where Ctrl-C stopped it, or
    // if what stopped it landed in its first local.
    let on_stop = "local went_on plays.on_stop(function() \
                   plays.window.rename{label = went_on and 'Went on' or 'Stopped'} end)";
    let ctrl_c = |cx: &mut TestAppContext, code: &str| {
        let mut plays = start_another_process(&path, &["do", "-e", code]);
        let waiting = wait_until(cx, |cx| label(cx).as_deref() == Some("Waiting"));
        if !waiting {
            plays.kill().ok();
            let output = plays.wait_with_output().unwrap();
            panic!(
                "{code}: it didn't get going: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let pid = i32::try_from(plays.id()).unwrap();
        // SAFETY: kill(2) of the child this test started, which hasn't
        // been waited for, so its id is still its own.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
        let mut exited = None;
        assert!(wait_until(cx, |_| {
            exited = plays.try_wait().unwrap();
            exited.is_some()
        }));
        let said = plays.wait_with_output().unwrap().stderr;
        (exited.unwrap(), String::from_utf8(said).unwrap())
    };
    for (stopped, code) in [
        ("a sleep", "plays.sleep(60) went_on = true"),
        ("a wait", "plays.wait('window.closed') went_on = true"),
        (
            "a timed wait",
            "plays.wait('window.closed', nil, 60) went_on = true",
        ),
        ("a program", "plays.exec{'sleep', '60'} went_on = true"),
        ("Lua running", "while true do end"),
        ("listening", "plays.on('window.closed', function() end)"),
    ] {
        TestAppContext::update(&cx, |cx| {
            api::run(
                api::Invocation::new("window.rename").with_args(json!({"label": "Going"})),
                cx,
            )
        })
        .unwrap();
        // On one line: the arguments reach the process a line each.
        let code = format!("{on_stop} plays.window.rename{{label = 'Waiting'}} {code}");
        let (exited, said) = ctrl_c(&mut cx, &code);
        assert_eq!(exited.code(), Some(130), "{stopped}: {exited:?} {said}");
        assert_eq!(label(&cx).as_deref(), Some("Stopped"), "{stopped}: {said}");
    }

    // With none to run, it ends at once, as Ctrl-C ends it.
    let (exited, said) = ctrl_c(
        &mut cx,
        "plays.window.rename{label = 'Waiting'} plays.sleep(60)",
    );
    assert_eq!(exited.signal(), Some(libc::SIGINT), "{exited:?} {said}");
}

#[gpui::test]
fn as_a_process_of_its_own_it_prints_and_exits_as_it_says(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("cli-process", cx);
    let (status, out, _) = another_process(&mut cx, &path, &["--json", "window", "ls"]);
    assert_eq!(status, Some(0));
    let listed: Value = serde_json::from_str(&out).unwrap_or_else(|err| panic!("{err}: {out}"));
    assert_eq!(listed[0]["id"], window_id(window, &cx).as_str());

    let (status, out, err) = another_process(&mut cx, &path, &["window", "inspect", "Kitchen"]);
    assert_eq!((status, out.as_str()), (Some(3), ""));
    assert!(
        err.starts_with("plays: ") && err.contains("Kitchen"),
        "{err}"
    );
    let (status, _, err) = another_process(&mut cx, &path, &["window", "frob"]);
    assert_eq!(status, Some(2), "{err}");
}
