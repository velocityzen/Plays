//! `plays.after` and `plays.every`, in scripts run against a Plays of the
//! test's own: when timers run, on which window, and how many a script
//! may have.

use std::time::{Duration, Instant};

use gpui::TestAppContext;

use crate::api::socket::tests::serving;
use crate::cli::tests::plays;

#[gpui::test]
fn a_timer_runs_once_its_seconds_are_up_and_every_one_until_cancelled(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-timers", cx);
    let code = r#"
        local ticks = 0
        local ticking
        ticking = plays.every(0.05, function()
          ticks = ticks + 1
          if ticks == 3 then
            ticking:cancel()
            plays.after(0.2, function() print("after", ticks) end)
          end
        end)
        plays.after(0.1, function() error("burnt", 0) end)
        print("first run over")
    "#;
    let began = Instant::now();
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (
            0,
            "first run over\nafter\t3\n",
            "running: 2 timers; Ctrl-C to stop\nplays: a timer failed: burnt\n"
        ),
        "{ran:?}"
    );
    let took = began.elapsed();
    assert!(took >= Duration::from_millis(350), "{took:?}");
    assert!(
        took < Duration::from_secs(10),
        "none going, it ends: {took:?}"
    );
}

#[gpui::test]
fn a_timers_calls_act_on_the_scripts_window_though_a_handler_made_it(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-timers-window", cx);
    let code = r#"
        plays.on("window.label_changed", function(ctx)
          plays.after(0, function()
            print(plays.window.inspect().id == plays.window_id, ctx.window_id ~= plays.window_id)
            plays.stop()
          end)
        end)
        local other = plays.window.new().window
        plays.window.rename{window_id = other, label = "Attic"}
    "#;
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (0, "true\ttrue\n", "listening: 1 handler; Ctrl-C to stop\n"),
        "{ran:?}"
    );
}

#[gpui::test]
fn a_tick_missed_while_busy_runs_once(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-timers-busy", cx);
    let code = r#"
        local ticks = 0
        plays.on("window.closed", print)
        plays.every(0.2, function() ticks = ticks + 1 end)
        plays.after(0.05, function()
          plays.sleep(0.5)
          plays.after(0.1, function() print(ticks); plays.stop() end)
        end)
    "#;
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (
            0,
            "1\n",
            "listening: 1 handler, running: 2 timers; Ctrl-C to stop\n"
        ),
        "{ran:?}"
    );
}

#[gpui::test]
fn a_script_has_at_most_so_many_timers(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-timers-most", cx);
    let code = r#"
        for _ = 1, 1024 do plays.after(60, print) end
        print(pcall(plays.every, 60, print))
        return "on"
    "#;
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(
        ran.failed(),
        (1, "plays: a script has at most 1024 timers"),
        "{ran:?}"
    );

    for (code, why) in [
        (
            "plays.after(-1, print)",
            "plays: plays.after can't wait -1 seconds",
        ),
        (
            "plays.every(0, print)",
            "plays: plays.every can't wait 0 seconds",
        ),
    ] {
        let ran = plays(&mut cx, &path, &["do", "-e", code]);
        assert_eq!(ran.failed(), (1, why), "{code}");
    }
}

#[gpui::test]
fn a_timer_slower_than_its_period_keeps_no_handler_waiting(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-timers-slow", cx);
    let code = r#"
        local ticks = 0
        plays.on("window.label_changed", function(_, event)
          print(event.label)
          plays.stop()
        end)
        plays.every(0.05, function()
          ticks = ticks + 1
          if ticks == 10 then print("starved"); plays.stop() end
          plays.sleep(0.1)
        end)
        plays.after(0.12, function() plays.window.rename{label = "Hall"} end)
    "#;
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    assert_eq!((ran.status, ran.out.as_str()), (0, "Hall\n"), "{ran:?}");
}
