//! `plays.sleep` and `plays.wait`, in scripts run against a Plays of the
//! test's own: what a wait hears, filtered, and for how long.

use std::time::{Duration, Instant};

use gpui::TestAppContext;
use mlua::Lua;
use serde_json::{Value, json};

use super::*;
use crate::api;
use crate::api::socket::tests::serving;
use crate::cli::tests::{plays, plays_while};
use crate::ui::testing::wait_until;

#[test]
fn a_table_filter_matches_the_fields_it_has_numbers_by_their_values() {
    let lua = Lua::new();
    let payload = json!({"block_id": "2", "time": 1.0, "label": null});
    for (code, matches) in [
        ("{}", true),
        ("{block_id = '2'}", true),
        ("{block_id = 2}", false),
        ("{time = 1}", true),
        ("{block_id = '2', time = 2}", false),
        ("{other = 1}", false),
    ] {
        let filter = Filter::read(&lua, lua.load(code).eval().unwrap()).unwrap();
        let matched = filter.matches(&payload, &LuaValue::Nil).unwrap();
        assert_eq!(matched, matches, "{code}");
    }
    let listed = Filter::read(&lua, lua.load("{1, 2}").eval().unwrap());
    assert!(listed.is_err(), "a list has no fields");
}

/// What the script `code` returned, as JSON, run against the Plays at
/// `path`, having said nothing on stderr.
fn returned(cx: &mut TestAppContext, path: &std::path::Path, code: &str) -> Value {
    let ran = plays(cx, path, &["--json", "do", "-e", code]);
    assert_eq!((ran.status, ran.err.as_str()), (0, ""), "{ran:?}");
    ran.json()
}

#[gpui::test]
fn a_wait_hears_what_the_call_before_it_did(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-wait-call", cx);
    let heard = returned(
        &mut cx,
        &path,
        r#"
        plays.window.rename{label = "Attic"}
        local heard = plays.wait("window.label_changed", nil, 5)
        return {label = heard.label, window = heard.window_id == plays.window_id}
        "#,
    );
    assert_eq!(heard, json!({"label": "Attic", "window": true}));
}

#[gpui::test]
fn a_wait_hears_nothing_from_before_the_call_before_it(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-wait-stale", cx);
    let heard = returned(
        &mut cx,
        &path,
        r#"
        plays.window.rename{label = "Attic"}
        plays.window.ls()
        local heard, why = plays.wait("window.label_changed", nil, 0.2)
        return {none = heard == nil, why = why}
        "#,
    );
    assert_eq!(heard, json!({"none": true, "why": "timeout"}));
}

#[gpui::test]
fn a_wait_hears_the_event_its_filter_matches(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-wait-filter", cx);
    let heard = returned(
        &mut cx,
        &path,
        r#"
        local heard = {}
        plays.window.rename{label = "Attic"}
        local passed, why = plays.wait("window.label_changed", {label = "Cellar"}, 0.2)
        heard.passed = {none = passed == nil, why = why}
        plays.window.rename{label = "Cellar"}
        local by_table = plays.wait(
          "window.label_changed", {label = "Cellar", window_id = plays.window_id}, 5)
        heard.by_table = by_table.label
        plays.window.rename{label = "Hall"}
        local asked = {}
        local by_function = plays.wait("window.label_changed", function(event)
          table.insert(asked, event.label)
          return event.label == "Hall"
        end, 5)
        heard.by_function = {by_function.label, asked}
        plays.window.rename{label = "Den"}
        local _, refused = plays.wait("window.label_changed", function() return false end, 0.2)
        plays.window.rename{label = "Study"}
        local _, unsaid = plays.wait("window.label_changed", function() end, 0.2)
        heard.refused = {refused, unsaid}
        return heard
        "#,
    );
    assert_eq!(
        heard,
        json!({
            "passed": {"none": true, "why": "timeout"},
            "by_table": "Cellar",
            "by_function": ["Hall", ["Hall"]],
            "refused": ["timeout", "timeout"],
        })
    );
}

#[gpui::test]
fn a_wait_hears_what_happens_elsewhere_as_it_waits(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-wait-elsewhere", cx);
    let ran = plays_while(
        &mut cx,
        &path,
        &[
            "--json",
            "do",
            "-e",
            r#"return plays.wait("window.label_changed", {label = "Hall"}, 10).label"#,
        ],
        |cx, time| {
            let label = if time % 2 == 0 {
                "label=Hall"
            } else {
                "label=Den"
            };
            plays(cx, &path, &["do", "window.rename", label]);
        },
    );
    assert_eq!((ran.status, ran.out.trim()), (0, "\"Hall\""), "{ran:?}");
}

#[gpui::test]
fn a_wait_for_where_a_file_is_hears_positions(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("lua-wait-position", cx);
    plays(&mut cx, &path, &["open", "tests/fixtures/clip.mp4"]);
    let opened = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, _| workspace.players().len())
            .unwrap()
            == 1
    });
    assert!(opened);
    let ran = plays_while(
        &mut cx,
        &path,
        &[
            "--json",
            "do",
            "-e",
            r#"
            plays.playback.play_pause{}
            local heard = plays.wait("playback.position", nil, 5)
            return heard.block_id == plays.block_id and heard.time ~= nil
            "#,
        ],
        |cx, _| {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(20));
        },
    );
    assert_eq!((ran.status, ran.out.trim()), (0, "true"), "{ran:?}");
}

#[gpui::test]
fn a_wait_says_so_when_plays_goes(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-wait-quit", cx);
    TestAppContext::update(&cx, api::record);
    let ran = plays_while(
        &mut cx,
        &path,
        &[
            "--json",
            "do",
            "-e",
            r#"
            plays.window.ls()
            local heard, why = plays.wait("window.label_changed")
            return {none = heard == nil, why = why}
            "#,
        ],
        |cx, time| {
            if time == 0 {
                let listed = wait_until(cx, |cx| {
                    let ran = cx.update(api::recorded);
                    ran.iter()
                        .any(|invocation| invocation.command == "window.ls")
                });
                assert!(listed, "it called, hearing already");
                cx.quit();
            }
        },
    );
    assert_eq!((ran.status, ran.err.as_str()), (0, ""), "{ran:?}");
    assert_eq!(
        ran.json(),
        json!({"none": true, "why": "Plays closed the connection"})
    );
}

#[gpui::test]
fn a_wait_says_what_it_cant_wait_for(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-wait-wrong", cx);
    for (code, why) in [
        (
            r#"plays.wait("window.label_changed", nil, -1)"#,
            "plays: plays.wait can't wait -1 seconds",
        ),
        (
            r#"plays.wait("window.label_changed", 5)"#,
            "plays: plays.wait's filter is a table or a function",
        ),
    ] {
        let ran = plays(&mut cx, &path, &["do", "-e", code]);
        assert_eq!(ran.failed(), (1, why), "{code}");
    }
}

#[gpui::test]
fn a_script_sleeps_as_long_as_it_says(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-sleep", cx);
    let began = Instant::now();
    let ran = plays(&mut cx, &path, &["do", "-e", "plays.sleep(0.3)"]);
    assert_eq!((ran.status, ran.err.as_str()), (0, ""), "{ran:?}");
    assert!(began.elapsed() >= Duration::from_millis(300));

    for (code, why) in [
        (
            "plays.sleep(-1)",
            "plays: plays.sleep can't sleep -1 seconds",
        ),
        (
            "plays.sleep(0/0)",
            "plays: plays.sleep can't sleep NaN seconds",
        ),
    ] {
        let ran = plays(&mut cx, &path, &["do", "-e", code]);
        assert_eq!(ran.failed(), (1, why), "{code}");
    }
}
