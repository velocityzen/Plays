//! `plays.on_stop`, in scripts run against a Plays of the test's own: the
//! function runs once as the script ends, however it ends, but as Plays
//! goes. Ctrl-C is in `cli::tests`, the script a process of its own.

use std::path::Path;
use std::time::Duration;

use gpui::TestAppContext;
use serde_json::{Value, json};

use crate::api::socket::tests::serving;
use crate::cli::tests::{plays, plays_while};

/// Renames the window "Stopped" and says so, as the script ends.
const ON_STOP: &str = r#"
plays.on_stop(function()
  plays.window.rename{label = "Stopped"}
  print("stopped")
end)
"#;

/// The label of the window in front.
fn label(cx: &mut TestAppContext, path: &Path) -> Value {
    let code = "return plays.window.inspect().label";
    plays(cx, path, &["--json", "do", "-e", code]).json()
}

/// Run `plays do OPTIONS -e CODE`, the app answering, as time goes.
fn script(
    cx: &mut TestAppContext,
    path: &Path,
    options: &[&str],
    code: &str,
) -> crate::cli::tests::Ran {
    let args = [&["do"], options, &["-e", code]].concat();
    plays_while(cx, path, &args, |cx, _| {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    })
}

#[gpui::test]
fn the_function_runs_once_as_the_script_ends_however_it_ends(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-stop", cx);
    let timer = "running: 1 timer; Ctrl-C to stop";
    for (options, code, status, said) in [
        (&[][..], "", 0, ""),
        (&[], "plays.stop(3)", 3, ""),
        (&[], "plays.after(0.1, function() end)", 0, timer),
        (
            &["--for", "0.3"],
            "plays.every(0.05, function() end)",
            0,
            timer,
        ),
        (&[], "error('broke', 0)", 1, "plays: broke"),
    ] {
        plays(&mut cx, &path, &["do", "window.rename", "label=Going"]);
        let ran = script(&mut cx, &path, options, &format!("{ON_STOP}\n{code}"));
        assert_eq!(
            (ran.status, ran.out.as_str(), ran.err.trim_end()),
            (status, "stopped\n", said),
            "{code}"
        );
        assert_eq!(label(&mut cx, &path), json!("Stopped"), "{code}");
    }
}

#[gpui::test]
fn the_function_given_last_runs_and_none_runs_after_nil(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-stop-given", cx);
    let ran = script(
        &mut cx,
        &path,
        &[],
        r#"
        plays.on_stop(function() print("first") end)
        plays.on_stop(function() print("last") end)
        "#,
    );
    assert_eq!((ran.status, ran.out.as_str()), (0, "last\n"), "{ran:?}");
    let ran = script(
        &mut cx,
        &path,
        &[],
        r#"
        plays.on_stop(function() print("stopped") end)
        plays.on_stop(nil)
        "#,
    );
    assert_eq!((ran.status, ran.out.as_str()), (0, ""), "{ran:?}");
}

#[gpui::test]
fn the_function_failing_fails_the_script(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-stop-fails", cx);
    let ran = script(
        &mut cx,
        &path,
        &[],
        "plays.on_stop(function() error('no', 0) end)",
    );
    assert_eq!(
        ran.failed(),
        (1, "plays: plays.on_stop's function failed: no")
    );
    // The script's own failure is what it ends with, its function's said.
    let ran = script(
        &mut cx,
        &path,
        &[],
        "plays.on_stop(function() error('no', 0) end) error('broke', 0)",
    );
    assert_eq!(
        ran.failed(),
        (
            1,
            "plays: plays.on_stop's function failed: no\nplays: broke"
        )
    );
}

#[gpui::test]
fn the_function_doesnt_run_as_plays_goes(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-stop-quit", cx);
    let code = format!("{ON_STOP}\nplays.on('window.label_changed', function() end)");
    let ran = plays_while(&mut cx, &path, &["do", "-e", &code], |cx, time| {
        if time == 10 {
            cx.quit();
        }
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    });
    assert_eq!(ran.out, "", "{ran:?}");
}
