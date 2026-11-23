//! The guides, `docs/lua.md` and `docs/lua-events.md`: every example in
//! them is Lua, and the scripts they name do as they say, run against a
//! Plays of the test's own.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{TestAppContext, WindowHandle};
use mlua::Lua;
use serde_json::{Value, json};

use crate::api::socket::tests::serving;
use crate::cli::tests::{plays, plays_while};
use crate::files;
use crate::ui::testing::{fixture, wait_until};
use crate::ui::workspace::Workspace;

const SCRIPTS: &str = include_str!("../../../../docs/lua.md");
const EVENTS: &str = include_str!("../../../../docs/lua-events.md");

/// The Lua examples in `guide`, in order.
fn examples(guide: &str) -> Vec<&str> {
    guide
        .split("```lua\n")
        .skip(1)
        .map(|rest| rest.split_once("```").expect("each example ends").0)
        .collect()
}

/// A folder of the test's own, gone as it ends.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("plays-{name}-files-{}", std::process::id()));
        let path = files::gone_as_test_ends(path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// The script named `name` in `guide`, on its first line, in the
    /// folder: its path.
    fn script(&self, guide: &str, name: &str) -> String {
        let named = format!("-- {name}:");
        let code = examples(guide)
            .into_iter()
            .find(|code| code.starts_with(&named))
            .unwrap_or_else(|| panic!("no {name}"));
        let path = self.0.join(name);
        fs::write(&path, code).unwrap();
        path.display().to_string()
    }

    fn path(&self, name: &str) -> String {
        self.0.join(name).display().to_string()
    }
}

/// Open `files` in the window, waiting till it has `blocks`.
fn open(
    cx: &mut TestAppContext,
    path: &Path,
    window: WindowHandle<Workspace>,
    files: &[&str],
    blocks: usize,
) {
    let args = [&["open"], files].concat();
    plays(cx, path, &args);
    let opened = wait_until(cx, |cx| {
        window
            .read_with(cx, |workspace, _| workspace.players().len())
            .unwrap()
            == blocks
    });
    assert!(opened, "{files:?} opened");
}

/// Whether `times` are two, each within a frame of `time`.
fn about(times: &Value, time: f64) -> bool {
    let near = |at: &Value| at.as_f64().is_some_and(|at| (at - time).abs() < 0.1);
    times
        .as_array()
        .is_some_and(|times| times.len() == 2 && times.iter().all(near))
}

/// What `code` returns, run against the Plays at `path`.
fn returned(cx: &mut TestAppContext, path: &Path, code: &str) -> Value {
    plays(cx, path, &["--json", "do", "-e", code]).json()
}

#[test]
fn every_example_in_the_guides_is_lua() {
    let lua = Lua::new();
    for (guide, text) in [("lua.md", SCRIPTS), ("lua-events.md", EVENTS)] {
        let examples = examples(text);
        assert!(examples.len() > 4, "{guide} has examples");
        for code in examples {
            let compiled = lua.load(code).set_name(guide).into_function();
            assert!(compiled.is_ok(), "{compiled:?}\n{code}");
        }
    }
}

#[gpui::test]
fn the_scripts_the_scripts_guide_names_do_as_it_says(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("lua-guide-scripts", cx);
    let folder = Folder::new("lua-guide-scripts");
    let (clip, other) = ("tests/fixtures/clip.mp4", "tests/fixtures/clip-one-key.mp4");
    open(&mut cx, &path, window, &[clip, other], 2);
    // Paused, so each stays where it's sought.
    plays(&mut cx, &path, &["do", "playback.pause_all"]);
    let volumes = "local v = {} for _, b in ipairs(plays.block.ls()) do \
                   v[#v + 1] = plays.block.inspect{block_id = b.id}.volume end return v";

    plays(
        &mut cx,
        &path,
        &["-b", "1", "do", "audio.set_volume", "volume=50"],
    );
    plays(
        &mut cx,
        &path,
        &["-b", "2", "do", "audio.set_volume", "volume=95"],
    );
    let louder = folder.script(SCRIPTS, "louder.lua");
    let ran = plays(&mut cx, &path, &["do", &louder]);
    assert_eq!((ran.status, ran.err.as_str()), (0, ""), "{ran:?}");
    assert_eq!(returned(&mut cx, &path, volumes), json!([60.0, 100.0]));

    let same = folder.script(SCRIPTS, "same-time.lua");
    let ran = plays(&mut cx, &path, &["--json", "do", &same, "time=1"]);
    assert!(about(&ran.json(), 1.0), "{ran:?}");
    let seek = folder.script(SCRIPTS, "seek.lua");
    let ran = plays(&mut cx, &path, &["do", &seek]);
    assert_eq!((ran.status, ran.err.as_str()), (0, ""), "{ran:?}");
    let positions = "local p = {} for _, b in ipairs(plays.block.ls()) do \
                     p[#p + 1] = plays.block.inspect{block_id = b.id}.position end return p";
    assert!(about(&returned(&mut cx, &path, positions), 0.0));

    let where_ = folder.script(SCRIPTS, "where.lua");
    let to = folder.path("where.json");
    let to_arg = format!("to={to}");
    let ran = plays(&mut cx, &path, &["do", "--for", "0.1", &where_, &to_arg]);
    assert_eq!(
        (ran.status, ran.err.as_str()),
        (0, "running: 1 timer; Ctrl-C to stop\n"),
        "{ran:?}"
    );
    let written: Value = serde_json::from_str(&fs::read_to_string(&to).unwrap()).unwrap();
    let field = |name: &str| -> Value {
        let blocks = written.as_array().unwrap();
        blocks.iter().map(|block| block[name].clone()).collect()
    };
    assert_eq!(field("title"), json!(["clip.mp4", "clip-one-key.mp4"]));
    assert!(about(&field("position"), 0.0), "{written}");

    let newest = folder.script(SCRIPTS, "newest.lua");
    let videos = Folder::new("lua-guide-newest");
    fs::copy(fixture("clip.mp4"), videos.path("old.mp4")).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    fs::copy(fixture("clip.mp4"), videos.path("new.mp4")).unwrap();
    let in_folder = format!("folder={}", videos.0.display());
    let ran = plays(&mut cx, &path, &["do", &newest, &in_folder]);
    assert_eq!((ran.status, ran.err.as_str()), (0, ""), "{ran:?}");
    let files = "local f = {} for _, b in ipairs(plays.block.ls()) do f[#f + 1] = b.file end \
                 return f";
    let opened = wait_until(&mut cx, |cx| {
        let files = returned(cx, &path, files);
        files.as_array().is_some_and(|files| files.len() == 3)
    });
    assert!(opened, "the newest opened");
    assert_eq!(
        returned(&mut cx, &path, files)[2],
        json!(videos.path("new.mp4"))
    );
}

#[gpui::test]
fn the_scripts_the_events_guide_names_do_as_it_says(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("lua-guide-events", cx);
    let folder = Folder::new("lua-guide-events");
    let (clip, other) = ("tests/fixtures/clip.mp4", "tests/fixtures/clip-one-key.mp4");
    open(&mut cx, &path, window, &[clip, other], 2);
    let playing = "local p = {} for _, b in ipairs(plays.block.ls()) do p[#p + 1] = b.playing end \
                   return p";
    let first = returned(&mut cx, &path, playing);
    if first[0] == false {
        plays(&mut cx, &path, &["-b", "1", "do", "playback.play_pause"]);
    }
    if first[1] == true {
        plays(&mut cx, &path, &["-b", "2", "do", "playback.play_pause"]);
    }
    assert_eq!(returned(&mut cx, &path, playing), json!([true, false]));

    // The second plays, once the script listens: the first pauses.
    let solo = folder.script(EVENTS, "solo.lua");
    let ran = plays_while(
        &mut cx,
        &path,
        &["do", "--for", "1.5", &solo],
        |cx, time| {
            if time == 10 {
                plays(cx, &path, &["-b", "2", "do", "playback.play_pause"]);
            }
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(20));
        },
    );
    assert_eq!(
        (ran.status, ran.err.as_str()),
        (0, "listening: 1 handler; Ctrl-C to stop\n"),
        "{ran:?}"
    );
    assert_eq!(returned(&mut cx, &path, playing), json!([false, true]));

    let opened = folder.script(EVENTS, "opened.lua");
    let log = folder.path("opened.log");
    let to = format!("to={log}");
    let ran = plays_while(
        &mut cx,
        &path,
        &["do", "--for", "1.5", &opened, &to],
        |cx, time| {
            if time == 10 {
                plays(cx, &path, &["open", "tests/fixtures/tone.m4a"]);
            }
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(20));
        },
    );
    assert_eq!(
        (ran.status, ran.err.as_str()),
        (0, "listening: 1 handler; Ctrl-C to stop\n"),
        "{ran:?}"
    );
    let logged = fs::read_to_string(&log).unwrap();
    let tone = fixture("tone.m4a").display().to_string();
    assert!(
        logged.lines().count() == 1 && logged.trim_end().ends_with(&format!(" {tone}")),
        "{logged:?}"
    );
}
