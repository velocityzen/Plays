//! `plays.on`, in scripts run against a Plays of the test's own: which
//! handlers run, for what, on which window, and how long a script listens.

use std::time::{Duration, Instant};

use gpui::TestAppContext;

use crate::api;
use crate::api::socket::tests::serving;
use crate::cli::show;
use crate::cli::tests::{playing_the_clip, plays, plays_while, step};
use crate::ui::testing::wait_until;

/// What stderr says as a script with `count` handlers starts listening.
fn listening(count: &str) -> String {
    format!("listening: {count}; Ctrl-C to stop\n")
}

#[gpui::test]
fn handlers_run_for_each_event_of_their_type_after_the_first_run(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-each", cx);
    let code = r#"
        plays.on("window.label_changed", function(ctx, event)
          print(event.label, ctx.window_id == plays.window_id, ctx.block_id)
          if event.label == "Den" then plays.stop(3) end
        end)
        plays.on("window.label_changed", function(_, event) print("then", event.label) end)
        plays.on("window.closed", function() print("closed") end)
        plays.window.rename{label = "Hall"}
        plays.window.rename{label = "Den"}
        plays.window.ls()
        print("first run over")
        return "returned"
    "#;
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (
            3,
            "first run over\n\"returned\"\nHall\ttrue\tnil\nthen\tHall\nDen\ttrue\tnil\n",
            listening("3 handlers").as_str()
        ),
    );
}

#[gpui::test]
fn a_handlers_calls_act_on_its_events_window(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-window", cx);
    let code = r#"
        plays.on("window.label_changed", function(ctx)
          print(plays.window.inspect().id == ctx.window_id, ctx.window_id ~= plays.window_id)
        end)
        plays.on("app.ui.off", function()
          print(plays.window.inspect().id == plays.window_id)
          plays.stop()
        end)
        local other = plays.window.new().window
        plays.window.rename{window_id = other, label = "Attic"}
        plays.app.ui.toggle()
    "#;
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str()),
        (0, "true\ttrue\ntrue\n"),
        "{ran:?}"
    );
}

#[gpui::test]
fn a_handler_failing_is_said_or_with_fail_fast_ends_the_script(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-fails", cx);
    let code = r#"
        plays.on("window.label_changed", function(_, event)
          if event.label == "Hall" then plays.on("window.closed", print) end
          if event.label == "Den" then error("burnt", 0) end
          if event.label == "Attic" then plays.stop(4) end
        end)
        plays.window.rename{label = "Hall"}
        plays.window.rename{label = "Den"}
        plays.window.rename{label = "Attic"}
    "#;
    let late = "plays: a window.label_changed handler failed: plays.on registers handlers \
                as the script first runs, not after\n";
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    let failed = "plays: a window.label_changed handler failed: burnt\n";
    let said = format!("{}{late}{failed}", listening("1 handler"));
    assert_eq!((ran.status, ran.err.as_str()), (4, said.as_str()));

    let ran = plays(
        &mut cx,
        &path,
        &["do", "--for", "10", "--fail-fast", "-e", code],
    );
    let said = format!("{}{late}", listening("1 handler"));
    assert_eq!((ran.status, ran.err.as_str()), (1, said.as_str()));
}

#[gpui::test]
fn a_handler_for_a_cue_runs_once_and_one_whose_block_closes_never(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("lua-on-cue", cx);
    playing_the_clip(&mut cx, &path, window);
    let code = r#"
        plays.block.playback.seek{time = 0}
        local block = plays.block_id
        plays.on("block." .. block .. ".position@0.5s", function(ctx, event)
          print("reached", ctx.block_id == block, event.time >= 0.5)
          plays.block.close{block_id = block}
        end)
        plays.on("block." .. block .. ".position@1.5s", function() print("never") end)
    "#;
    let ran = plays_while(
        &mut cx,
        &path,
        &["do", "--for", "10", "-e", code],
        |cx, _| step(cx),
    );
    let block = window
        .read_with(&cx, |workspace, _| workspace.players().len())
        .unwrap();
    assert_eq!(block, 0, "closed");
    let (said, closed) = ran.err.split_once('\n').unwrap_or_default();
    assert_eq!(
        (ran.status, ran.out.as_str(), said),
        (
            0,
            "reached\ttrue\ttrue\n",
            listening("2 handlers").trim_end()
        ),
        "{ran:?}"
    );
    assert!(
        closed.starts_with("plays: block.")
            && closed.ends_with(".position@1.5s: the block closed before it\n"),
        "no handler left, it ended: {ran:?}"
    );
}

#[gpui::test]
fn a_script_listens_as_long_as_for_says(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-for", cx);
    let code = r#"plays.on("window.closed", print)"#;
    let began = Instant::now();
    let ran = plays(&mut cx, &path, &["do", "--for", "0.3s", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (0, "", listening("1 handler").as_str())
    );
    assert!(began.elapsed() >= Duration::from_millis(300));

    let ran = plays(&mut cx, &path, &["do", "--for", "1", "-e", "return 1"]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (0, "1\n", ""),
        "no handlers"
    );
}

#[gpui::test]
fn a_script_listens_until_plays_quits(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-quit", cx);
    TestAppContext::update(&cx, api::record);
    let ran = plays_while(
        &mut cx,
        &path,
        &[
            "do",
            "-e",
            r#"
            plays.on("window.active_changed", function(ctx, event)
              print("in front", ctx.window_id, event.window_id)
            end)
            plays.on("app.quit", function() print("quitting") end)
            plays.window.ls()
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
    assert_eq!(
        (ran.status, ran.out.as_str()),
        (0, "in front\tnil\tnil\nquitting\n"),
        "{ran:?}"
    );
}

#[gpui::test]
fn with_w_a_script_hears_that_window_until_it_closes(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-closes", cx);
    TestAppContext::update(&cx, api::record);
    let other = plays(&mut cx, &path, &["--json", "do", "window.new"]).json()["window"].clone();
    let other: &'static str = show::text(&other).leak();
    let began = Instant::now();
    let ran = plays_while(
        &mut cx,
        &path,
        &[
            "-w",
            other,
            "do",
            "--for",
            "10",
            "-e",
            r#"
            plays.on("window.label_changed", function(_, event) print(event.label) end)
            plays.on("window.closed", function() print("closed") end)
            plays.window.ls()
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
                plays(cx, &path, &["-w", "1", "do", "window.rename", "label=Hall"]);
                plays(
                    cx,
                    &path,
                    &["-w", other, "do", "window.rename", "label=Den"],
                );
                plays(cx, &path, &["window", "close", other]);
            }
        },
    );
    assert_eq!(
        (ran.status, ran.out.as_str()),
        (0, "Den\nclosed\n"),
        "{ran:?}"
    );
    assert!(began.elapsed() < Duration::from_secs(10), "not --for's end");
}

#[gpui::test]
fn a_handler_for_where_a_file_is_hears_positions(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("lua-on-position", cx);
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
            "do",
            "--for",
            "10",
            "-e",
            r#"
            plays.on("block.playback.position", function(ctx, event)
              print(ctx.block_id == plays.block_id, event.time ~= nil)
              plays.stop()
            end)
            plays.block.playback.play_pause{}
            "#,
        ],
        |cx, _| {
            cx.executor().advance_clock(Duration::from_millis(100));
            cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(20));
        },
    );
    assert_eq!(
        (ran.status, ran.out.as_str()),
        (0, "true\ttrue\n"),
        "{ran:?}"
    );
}

#[gpui::test]
fn a_wait_in_a_handler_holds_the_others_and_hears_from_its_event_on(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-on-holds", cx);
    let code = r#"
        plays.on("window.label_changed", function(_, event)
          print(event.label)
          if event.label == "Hall" then
            print("waited", plays.wait("window.label_changed", {label = "Attic"}, 5).label)
          end
          if event.label == "Cellar" then plays.stop() end
        end)
        plays.window.rename{label = "Hall"}
        plays.window.rename{label = "Den"}
        plays.window.rename{label = "Attic"}
        plays.window.rename{label = "Cellar"}
    "#;
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str()),
        (0, "Hall\nwaited\tAttic\nCellar\n"),
        "{ran:?}"
    );
}
