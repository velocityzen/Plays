use std::fs;
use std::path::{Path, PathBuf};

use gpui::TestAppContext;
use serde_json::json;

use super::*;
use crate::api::socket::tests::serving;
use crate::cli::Unanswered;
use crate::cli::tests::{command_line, plays, plays_reading, window_id};
use crate::ui::testing::wait_until;

/// `code` kept as `name` beside the test's socket, at `path`: its path.
fn kept(path: &Path, name: &str, code: &str) -> PathBuf {
    let file = path.parent().unwrap().join(name);
    fs::write(&file, code).unwrap();
    file
}

#[test]
fn a_name_is_a_script_when_it_ends_in_lua_has_a_slash_or_is_a_dash() {
    for name in ["seek.lua", "./seek", "scripts/seek", "-", "/tmp/a.lua"] {
        assert!(is_script(name), "{name}");
    }
    for name in ["playback.seek", "window.ls", "lua", "seek.luac"] {
        assert!(!is_script(name), "{name}");
    }
}

#[test]
fn a_script_must_be_there_and_its_arguments_given_once_as_json() {
    let ran = command_line(None, Unanswered::Fails, &["do", "missing.lua"]);
    assert_eq!(ran.failed(), (3, "plays: no script missing.lua"));
    let ran = command_line(
        None,
        Unanswered::Fails,
        &["do", "-e", "return 1", "--args", "{}", "n=1"],
    );
    assert_eq!(
        ran.failed(),
        (
            2,
            "plays: give the arguments as KEY=VALUE or with --args, not both"
        )
    );
    let ran = command_line(
        None,
        Unanswered::Fails,
        &["do", "-e", "return 1", "--args", "[1"],
    );
    assert_eq!(ran.status, 2, "{ran:?}");
    assert!(
        ran.err.starts_with("plays: the arguments aren't JSON: "),
        "{ran:?}"
    );
    let ran = command_line(
        None,
        Unanswered::Fails,
        &["do", "-e", "1", "--args", "@missing.json"],
    );
    assert_eq!(ran.status, 2, "{ran:?}");
    assert!(
        ran.err.starts_with("plays: can't read missing.json: "),
        "{ran:?}"
    );
    let ran = command_line(None, Unanswered::Fails, &["do"]);
    assert_eq!(ran.status, 2, "neither a command nor -e: {ran:?}");
}

#[gpui::test]
fn a_script_runs_from_a_file_code_or_stdin_and_prints_what_it_returns(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-run", cx);
    let ran = plays(&mut cx, &path, &["do", "-e", "return 1 + 1"]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (0, "2\n", "")
    );
    let ran = plays_reading(&mut cx, &path, &["do", "-"], b"return {a = 'b'}");
    assert_eq!((ran.status, ran.out.as_str()), (0, "{\"a\":\"b\"}\n"));
    let ran = plays(&mut cx, &path, &["--json", "do", "-e", "return {1, 2}"]);
    assert_eq!(ran.out, "[\n  1,\n  2\n]\n", "indented");
    let ran = plays(&mut cx, &path, &["do", "-e", "local nothing = 1"]);
    assert_eq!((ran.status, ran.out.as_str()), (0, ""), "nothing returned");
    let ran = plays(&mut cx, &path, &["do", "-e", "plays.window.ls()"]);
    assert_eq!((ran.status, ran.out.as_str()), (0, ""), "nor a call's");
    let ran = plays(&mut cx, &path, &["do", "-e", "return nil, 2"]);
    assert_eq!((ran.status, ran.out.as_str()), (0, ""), "nil first");

    // Its arguments: typed as they're written, or as JSON, or a file's.
    let script = kept(&path, "args.lua", "return plays.args");
    let script = script.to_str().unwrap();
    let ran = plays(
        &mut cx,
        &path,
        &["do", script, "n=3", "half=0.5", "on=true", "name=x"],
    );
    assert_eq!(
        ran.json(),
        json!({"n": 3, "half": 0.5, "on": true, "name": "x"}),
        "{ran:?}"
    );
    let ran = plays(
        &mut cx,
        &path,
        &["do", script, "--args", r#"{"list": [1, "a"]}"#],
    );
    assert_eq!(ran.json(), json!({"list": [1, "a"]}), "{ran:?}");
    let file = kept(&path, "args.json", r#"{"from": "file"}"#);
    let at_file = format!("@{}", file.display());
    let ran = plays(&mut cx, &path, &["do", script, "--args", &at_file]);
    assert_eq!(ran.json(), json!({"from": "file"}), "{ran:?}");
    let ran = plays(&mut cx, &path, &["do", "-e", "return plays.args", "n=1"]);
    assert_eq!(ran.json(), json!({"n": 1}), "after -e's code: {ran:?}");
    let ran = plays(&mut cx, &path, &["do", script]);
    assert_eq!(ran.json(), json!({}), "none: {ran:?}");

    // require finds what's beside it.
    kept(&path, "twice.lua", "return function(n) return n * 2 end");
    let script = kept(&path, "uses.lua", "return require('twice')(plays.args.n)");
    let ran = plays(&mut cx, &path, &["do", script.to_str().unwrap(), "n=21"]);
    assert_eq!((ran.status, ran.out.as_str()), (0, "42\n"), "{ran:?}");
}

#[gpui::test]
fn json_and_lua_values_go_both_ways(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-json", cx);
    let returned = |cx: &mut TestAppContext, code: &str| {
        let ran = plays(cx, &path, &["do", "-e", code]);
        assert_eq!(ran.status, 0, "{code}: {ran:?}");
        ran.json()
    };
    assert_eq!(
        returned(
            &mut cx,
            "return {i = 3, f = 1.5, w = 2.0, s = 'x', t = true}"
        ),
        json!({"i": 3, "f": 1.5, "w": 2.0, "s": "x", "t": true}),
        "integers stay integers, and floats floats"
    );
    assert_eq!(returned(&mut cx, "return {}"), json!({}), "an empty table");
    assert_eq!(
        returned(&mut cx, "return {plays.null, 1}"),
        json!([null, 1]),
        "plays.null is null"
    );
    // What Plays hands back: null as nil, and lists, empty ones too, as
    // lists.
    let code = "local w = plays.window.ls()[1]; return {w.label == nil, #plays.window.ls()}";
    assert_eq!(returned(&mut cx, code), json!([true, 1]));
    assert_eq!(returned(&mut cx, "return plays.block.ls()"), json!([]));
    let code = "local b = plays.block.ls(); b[1] = 'x'; return b";
    assert_eq!(returned(&mut cx, code), json!(["x"]));

    let ran = plays(&mut cx, &path, &["do", "-e", "return print"]);
    assert_eq!(ran.status, 1, "{ran:?}");
    assert!(
        ran.err.starts_with("plays: what it returned isn't JSON: "),
        "{ran:?}"
    );
}

#[gpui::test]
fn a_script_calls_commands_on_its_window_and_block(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("lua-call", cx);
    let id = window_id(window, &cx);
    let ran = plays(
        &mut cx,
        &path,
        &["do", "-e", "return {plays.window_id, plays.block_id}"],
    );
    assert_eq!(ran.json(), json!([id]), "no block yet: {ran:?}");

    // By namespace, or by id; a namespace's own command too.
    let code = "plays.window.rename{label = 'Stage'}; return plays.call('window.inspect').label";
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(ran.out, "\"Stage\"\n", "{ran:?}");
    let code = "local found, why = plays.call('playback.nope'); return {found == nil, why}";
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(ran.json(), json!([true, "no command playback.nope"]));
    // The value alone, as Lua's own functions hand it back; nil and why
    // only as it fails.
    let code = "return {select('#', plays.window.ls()), select('#', plays.call('playback.nope'))}";
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(ran.json(), json!([1, 2]), "{ran:?}");
    let code = "local _, why = plays.playback.play_pause(); return why";
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(
        ran.json(),
        json!(format!("no block in window {id}")),
        "{ran:?}"
    );
    let code = "local _, why = plays.window.inspect{window_id = 'Kitchen'}; return why";
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(
        ran.json(),
        json!("no window Kitchen"),
        "named in its arguments"
    );

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
    let block = |cx: &mut TestAppContext, args: &[&str]| {
        let args = [args, &["--json", "do", "block.inspect"]].concat();
        plays(cx, &path, &args).json()["id"].clone()
    };
    let (front, other) = (block(&mut cx, &[]), block(&mut cx, &["-b", "2"]));
    assert_ne!(front, other);
    let code = "return {plays.window_id, plays.block_id, plays.block.inspect().id}";
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(
        ran.json(),
        json!([id, front, front]),
        "the block keys go to"
    );
    let ran = plays(&mut cx, &path, &["-b", "2", "do", "-e", code]);
    assert_eq!(ran.json(), json!([id, other, other]), "the one -b names");
    let code = "return plays.block.inspect{block_id = 2}.id";
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(ran.json(), other, "named in its arguments");
    let ran = plays(&mut cx, &path, &["-w", "Kitchen", "do", "-e", "return 1"]);
    assert_eq!(ran.failed(), (3, "plays: no window Kitchen"));

    // A window's command on the script's window, though another's in
    // front; none with no window.
    let other = plays(&mut cx, &path, &["--json", "do", "window.new"]).json()["window"].clone();
    let code = "return {plays.window_id, plays.window.inspect().id}";
    let ran = plays(&mut cx, &path, &["-w", "1", "do", "-e", code]);
    assert_eq!(ran.json(), json!([id, id]), "{other} in front");
    plays(
        &mut cx,
        &path,
        &["window", "close", "1", &show::text(&other)],
    );
    let code = "return {plays.window_id == nil, plays.block_id == nil}";
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(ran.json(), json!([true, true]), "{ran:?}");
}

#[gpui::test]
fn a_script_prints_logs_stops_and_fails(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-stop", cx);
    let code = "print('a', 1, nil); print(); plays.log('note', 2); return 3";
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (0, "a\t1\tnil\n\n3\n", "note\t2\n")
    );
    for (code, status) in [
        ("plays.stop()", 0),
        ("plays.stop(3); return 1", 3),
        ("print(pcall(plays.stop, 4)); return 1", 4),
        ("while true do pcall(plays.stop, 5) end", 5),
        ("pcall(function() plays.stop(6) end); return 1", 6),
    ] {
        let ran = plays(&mut cx, &path, &["do", "-e", code]);
        assert_eq!(
            (ran.status, ran.out.as_str(), ran.err.as_str()),
            (status, "", ""),
            "{code}"
        );
    }
    for (code, said) in [
        (
            "plays.stop(256)",
            "plays.stop takes an exit status from 0 to 255",
        ),
        (
            "plays.stop(-1)",
            "plays.stop takes an exit status from 0 to 255",
        ),
        ("error('boom')", "(command line):1: boom"),
        ("return +", "(command line):1: unexpected symbol near '+'"),
        (
            "return io.popen('ls')",
            "(command line):1: attempt to call a nil value (field 'popen')",
        ),
        (
            "os.execute('ls')",
            "(command line):1: attempt to call a nil value (field 'execute')",
        ),
        (
            "return debug.traceback()",
            "(command line):1: attempt to index a nil value (global 'debug')",
        ),
    ] {
        let ran = plays(&mut cx, &path, &["do", "-e", code]);
        assert_eq!(
            ran.failed(),
            (1, format!("plays: {said}").as_str()),
            "{code}"
        );
    }
    let script = kept(&path, "fails.lua", "\nerror('here')");
    let ran = plays(&mut cx, &path, &["do", script.to_str().unwrap()]);
    assert_eq!(ran.status, 1, "{ran:?}");
    // Its path, its start cut if it's long, as Lua has it.
    assert!(ran.err.ends_with("/fails.lua:2: here\n"), "{ran:?}");
    let ran = plays_reading(&mut cx, &path, &["do", "-"], b"error('piped')");
    assert_eq!(ran.failed(), (1, "plays: stdin:1: piped"));
}

#[gpui::test]
fn a_wait_longer_than_anything_lasts_as_long_as_it_takes(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-forever", cx);
    let code = r#"
        plays.after(1e19, function() end):cancel()
        print(plays.exec({"true"}, {timeout = 1e19}).ok)
        plays.window.rename{label = "Forever"}
        print(plays.wait("window.label_changed", nil, 1e19).label)
        plays.after(0.05, function() print("ran") end)
    "#;
    let ran = plays(&mut cx, &path, &["do", "--for", "1e19", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str()),
        (0, "true\nForever\nran\n"),
        "{ran:?}"
    );
}
