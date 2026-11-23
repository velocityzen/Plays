//! `plays.exec` and `plays.exec.start`, in scripts run against a Plays of
//! the test's own: what a program wrote and how it ended, its options, its
//! output to functions as it comes, and when it's stopped.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gpui::TestAppContext;

use crate::api::socket::tests::serving;
use crate::cli::tests::plays;

#[gpui::test]
fn a_program_runs_with_no_shell_and_says_what_it_wrote_and_how_it_ended(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-exec", cx);
    let code = r#"
        local ended = plays.exec{"sh", "-c", 'printf "%s" "$1"; printf oops >&2; exit 3', "sh", "a b"}
        print(ended.ok, ended.code, ended.stdout, ended.stderr)
        local fine = plays.exec{"true"}
        print(fine.ok, fine.code, fine.stdout == "", fine.stderr == "")
        print(plays.exec{"no-such-program-for-plays"})
        print(select('#', plays.exec{"true"}), select('#', plays.exec.start{"true"}),
          select('#', plays.exec{"no-such-program-for-plays"}))
    "#;
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (
            0,
            "false\t3\ta b\toops\ntrue\t0\ttrue\ttrue\n\
             nil\tcan't run no-such-program-for-plays: No such file or directory (os error 2)\n\
             1\t1\t2\n",
            ""
        ),
        "{ran:?}"
    );
}

#[gpui::test]
fn a_program_runs_in_the_folder_with_the_variables_and_for_as_long_as_given(
    cx: &mut TestAppContext,
) {
    let (path, _, mut cx) = serving("lua-exec-options", cx);
    let code = r#"
        print(plays.exec({"pwd"}, {cwd = "/"}).stdout)
        local env = {PLAYS_GIVEN = "given", HOME = false}
        local said = plays.exec({"sh", "-c", 'printf "%s %s" "$PLAYS_GIVEN" "${HOME-none}"'}, {env = env})
        print(said.stdout)
        local slow = plays.exec({"sleep", "5"}, {timeout = 0.1})
        print(slow.ok, slow.code)
        local holding = plays.exec({"sh", "-c", "sleep 5 & wait"}, {timeout = 0.1})
        print(holding.ok, holding.code)
        print(plays.exec({"pwd"}, {cwd = "/no/such/folder"}))
    "#;
    let began = Instant::now();
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (
            0,
            "/\n\ngiven none\nfalse\tnil\nfalse\tnil\n\
             nil\tcan't run pwd in /no/such/folder: no such folder\n",
            ""
        ),
        "{ran:?}"
    );
    let took = began.elapsed();
    assert!(took < Duration::from_secs(4), "killed, let go: {took:?}");

    for (code, why) in [
        (
            "plays.exec{}",
            "plays: plays.exec takes the program to run, then its arguments",
        ),
        (
            "plays.exec({'true'}, {timeout = -1})",
            "plays: plays.exec can't wait -1 seconds",
        ),
        (
            "plays.exec.start({'true'}, {env = {GIVEN = 1}})",
            "plays: plays.exec.start's env sets GIVEN to a string, or with false removes it",
        ),
    ] {
        let ran = plays(&mut cx, &path, &["do", "-e", code]);
        assert_eq!(ran.failed(), (1, why), "{code}");
    }
}

#[gpui::test]
fn a_programs_output_goes_to_its_function_as_it_comes_as_the_script_waits(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-exec-start", cx);
    // Each program goes on once a function has heard it: as it comes, and
    // as the script waits for another.
    let code = r#"
        local function flag()
          local named = os.tmpname()
          io.open(named, "w"):close()
          return named
        end
        local held, early = flag(), flag()
        local pieces = {}
        local program = assert(plays.exec.start(
          {"sh", "-c", 'printf one; while [ -e "$1" ]; do sleep 0.01; done; printf two; printf oops >&2', "sh", held},
          {stdout = function(piece) pieces[#pieces + 1] = piece; os.remove(held) end, timeout = 5}
        ))
        local ended = program:wait()
        print(table.concat(pieces, "+"), ended.ok, ended.stdout == "", ended.stderr)

        local heard
        plays.exec.start({"printf", "early"}, {stdout = function(piece) heard = piece; os.remove(early) end})
        local waited = plays.exec({"sh", "-c", 'while [ -e "$1" ]; do sleep 0.01; done', "sh", early}, {timeout = 5})
        print(heard, waited.ok)
    "#;
    let ran = plays(&mut cx, &path, &["do", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (0, "one+two\ttrue\ttrue\toops\nearly\ttrue\n", ""),
        "{ran:?}"
    );
}

#[gpui::test]
fn a_program_is_stopped_as_the_part_that_started_it_ends_unless_detached(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-exec-parts", cx);
    // A timer's, then a handler's, the handler hearing what the timer did;
    // what a program stopped wrote is let go, its function never called.
    let code = r#"
        local top = plays.exec.start{"sleep", "5"}
        local detached = plays.exec.start({"sh", "-c", "sleep 0.3; printf on"}, {detach = true})
        local timed
        plays.after(0, function()
          timed = plays.exec.start{"sleep", "5"}
          plays.exec.start({"sh", "-c", "printf late; sleep 5"}, {stdout = function() print("late") end})
          plays.sleep(0.2)
          plays.window.rename{label = "Hall"}
        end)
        plays.on("window.label_changed", function()
          local handled = plays.exec.start{"sleep", "5"}
          plays.after(0, function()
            print(top:wait().code, timed:wait().code, handled:wait().code, detached:wait().stdout)
            plays.stop()
          end)
        end)
    "#;
    let began = Instant::now();
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (
            0,
            "nil\tnil\tnil\ton\n",
            "listening: 1 handler, running: 1 timer; Ctrl-C to stop\n"
        ),
        "{ran:?}"
    );
    let took = began.elapsed();
    assert!(took < Duration::from_secs(4), "stopped: {took:?}");
}

#[gpui::test]
fn a_detached_programs_output_goes_to_its_functions_as_the_script_runs_on(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-exec-detached", cx);
    let code = r#"
        plays.exec.start({"sh", "-c", "printf oops >&2; sleep 0.1; printf hello"}, {
          detach = true,
          stdout = function(piece) print("heard", piece); plays.stop() end,
          stderr = function() error("burnt", 0) end,
        })
        plays.every(10, print)
    "#;
    let ran = plays(&mut cx, &path, &["do", "--for", "10", "-e", code]);
    assert_eq!(
        (ran.status, ran.out.as_str(), ran.err.as_str()),
        (
            0,
            "heard\thello\n",
            "running: 1 timer; Ctrl-C to stop\nplays: sh's stderr function failed: burnt\n"
        ),
        "{ran:?}"
    );
}

#[gpui::test]
fn a_scripts_end_stops_its_programs_detached_too(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("lua-exec-end", cx);
    let file = std::env::temp_dir().join(format!("plays-exec-end-{}", std::process::id()));
    std::fs::remove_file(&file).ok();
    let named = file.display();
    let code = format!(
        r#"
        plays.exec.start({{"sh", "-c", 'echo $$ > "$1"; exec sleep 30', "sh", "{named}"}}, {{detach = true}})
        local pid
        repeat
          plays.sleep(0.01)
          local file = io.open("{named}")
          if file then pid = file:read("n"); file:close() end
        until pid
        "#
    );
    let ran = plays(&mut cx, &path, &["do", "-e", &code]);
    assert_eq!((ran.status, ran.err.as_str()), (0, ""), "{ran:?}");
    let pid = std::fs::read_to_string(&file).unwrap();
    std::fs::remove_file(&file).ok();
    let there = Command::new("kill")
        .args(["-0", pid.trim()])
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!there.success(), "{pid} is gone");
}
