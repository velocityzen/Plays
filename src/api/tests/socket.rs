//! The socket: taking it, or finding a Plays there, or one gone; what a
//! client asks and what comes back; files handed over by a Plays started
//! with them; events heard; and starting Plays when none answers. Each
//! client runs on a thread of its own, as another process would, while
//! the test runs the app.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use serde_json::{Value, json};

use super::client::{self, Client, Failure};
use super::protocol::{Ask, Heard};
use super::server::LONGEST;
use super::*;
use crate::api::{self, Error, Invocation, Target};
use crate::settings::Settings;
use crate::ui::testing::{self, fixture, wait_until};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

/// A socket of a test's own, in a folder not made yet, and removed as the
/// test ends, pass or fail. Short: a socket's path has room for about a
/// hundred bytes.
pub(crate) fn socket(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("plays-{name}-{}", std::process::id()));
    fs::remove_dir_all(&folder).ok();
    MADE.with(|made| made.0.borrow_mut().push(folder.clone()));
    folder.join(NAME)
}

thread_local! {
    /// The folders made for the test on this thread: each test runs on a
    /// thread of its own.
    static MADE: Made = const { Made(RefCell::new(Vec::new())) };
}

/// Folders removed as the thread that made them ends.
struct Made(RefCell<Vec<PathBuf>>);

impl Drop for Made {
    fn drop(&mut self) {
        for folder in self.0.get_mut().drain(..) {
            // Opened first, as a test may have failed with it shut.
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).ok();
            fs::remove_dir_all(&folder).ok();
        }
    }
}

#[test]
fn a_tests_folders_are_removed_as_it_ends() {
    let made = thread::spawn(|| {
        let open = socket("made");
        let shut = socket("shut");
        for path in [&open, &shut] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let folder = shut.parent().unwrap();
        fs::set_permissions(folder, fs::Permissions::from_mode(0o000)).unwrap();
        [open, shut]
    })
    .join()
    .unwrap();
    for path in made {
        let folder = path.parent().unwrap();
        assert!(!folder.exists(), "{} left", folder.display());
    }
}

/// The app, with its first window in front, answering on a socket of its
/// own, control from the command line turned on in Settings.
pub(crate) fn serving(
    name: &str,
    cx: &mut TestAppContext,
) -> (PathBuf, WindowHandle<Workspace>, VisualTestContext) {
    let (window, mut cx) = testing::start(cx);
    let path = socket(name);
    let Ok(Claimed::Ours(listener)) = claim(&path) else {
        panic!("taking the socket")
    };
    TestAppContext::update(&cx, |cx| serve(listener, cx));
    allow_control(&mut cx, true);
    (path, window, cx)
}

/// Turn control from the command line on or off, as Settings does.
pub(crate) fn allow_control(cx: &mut TestAppContext, on: bool) {
    cx.update(|cx| Settings::update(cx, false, |s| s.command_line_control = on));
    cx.run_until_parked();
}

/// Run `talk` on a thread of its own, as another process would, while
/// the app answers it; what it came to.
pub(crate) fn talk<T: Send + 'static>(
    cx: &mut TestAppContext,
    talk: impl FnOnce() -> T + Send + 'static,
) -> T {
    let talking = thread::spawn(talk);
    assert!(
        wait_until(cx, |_| talking.is_finished()),
        "the client was answered"
    );
    talking.join().expect("the client")
}

/// Listen on a thread of its own: `listen` subscribes, and the test goes
/// on once it has, then hears what it hears; what it heard.
fn listening<T: Send + 'static>(
    cx: &mut TestAppContext,
    path: &Path,
    listen: impl FnOnce(&mut Client) -> T + Send + 'static,
    heard: impl FnOnce(&mut Client) -> Vec<(String, Value)> + Send + 'static,
) -> (T, thread::JoinHandle<Vec<(String, Value)>>) {
    let path = path.to_owned();
    let (subscribed, answered) = mpsc::channel();
    let hearing = thread::spawn(move || {
        let mut client = Client::connect(&path).expect("connecting");
        subscribed.send(listen(&mut client)).ok();
        heard(&mut client)
    });
    let mut answer = None;
    let ready = wait_until(cx, |_| {
        answer = answered.try_recv().ok();
        answer.is_some()
    });
    assert!(ready, "it subscribed");
    (answer.expect("its answer"), hearing)
}

/// The events heard until one of `last`'s kind.
fn until(client: &mut Client, last: &str) -> Vec<(String, Value)> {
    let mut heard = Vec::new();
    loop {
        let event = client.next_event().expect("an event");
        let done = event.0 == last;
        heard.push(event);
        if done {
            return heard;
        }
    }
}

fn kinds(heard: &[(String, Value)]) -> Vec<&str> {
    heard.iter().map(|(kind, _)| kind.as_str()).collect()
}

fn window_id(window: WindowHandle<Workspace>, cx: &VisualTestContext) -> String {
    window
        .read_with(cx, |workspace, _| workspace.id().to_string())
        .unwrap()
}

fn in_window(name: &str) -> Target {
    Target {
        window: Some(name.into()),
        tile: None,
    }
}

/// Its exit status and what it says.
fn said(failure: Failure) -> (u8, String) {
    (failure.exit_status(), failure.to_string())
}

#[test]
fn takes_the_socket_in_a_folder_only_this_user_can_open() {
    let path = socket("fresh");
    let Ok(Claimed::Ours(_listener)) = claim(&path) else {
        panic!("taking the socket")
    };
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(path.parent().unwrap()), 0o700);
    assert_eq!(mode(&path), 0o600);
    assert!(UnixStream::connect(&path).is_ok(), "it listens");
}

#[test]
fn finds_a_plays_answering_there_and_leaves_its_socket_be() {
    let path = socket("running");
    let Ok(Claimed::Ours(_listener)) = claim(&path) else {
        panic!("taking the socket")
    };
    assert!(matches!(claim(&path), Ok(Claimed::Running)));
    assert!(
        UnixStream::connect(&path).is_ok(),
        "the first still listens"
    );
}

#[test]
fn a_socket_left_by_a_plays_that_crashed_is_taken_over() {
    let path = socket("stale");
    let Ok(Claimed::Ours(listener)) = claim(&path) else {
        panic!("taking the socket")
    };
    // Crashed: gone without removing it.
    drop(listener);
    assert!(path.exists());
    assert!(UnixStream::connect(&path).is_err(), "no one answers there");

    let Ok(Claimed::Ours(_listener)) = claim(&path) else {
        panic!("taking it over")
    };
    assert!(UnixStream::connect(&path).is_ok(), "this one listens there");
}

#[test]
fn what_isnt_a_socket_is_left_where_the_socket_would_be() {
    let path = socket("file");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "mine").unwrap();
    assert!(claim(&path).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "mine");
}

#[test]
fn only_a_socket_is_removed() {
    let path = socket("removed");
    let Ok(Claimed::Ours(listener)) = claim(&path) else {
        panic!("taking the socket")
    };
    drop(listener);
    remove(&path).unwrap();
    assert!(!path.exists());
    remove(&path).expect("nothing there to remove");

    fs::write(&path, "mine").unwrap();
    let err = remove(&path).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read_to_string(&path).unwrap(), "mine");
}

#[test]
fn tests_run_alone_without_a_socket() {
    assert_eq!(path(), None);
    assert!(matches!(
        start(&[OsString::from("/media/film.mkv")]),
        Start::Runs(None)
    ));
}

#[test]
fn a_socket_named_in_the_environment_is_taken_there() {
    let named = |name: &str| path_named(Some(name.into()));
    assert_eq!(
        named("/run/other/plays.sock"),
        Some("/run/other/plays.sock".into())
    );
    assert_eq!(named(""), None, "an empty one names none");
    assert_eq!(path_named(None), None);
}

#[test]
fn two_plays_starting_at_once_take_the_socket_in_turn() {
    let path = socket("turns");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Another, looking.
    let lock = fs::File::create(path.with_file_name(LOCK)).unwrap();
    lock.lock().unwrap();
    let claiming = path.clone();
    let claimed = thread::spawn(move || matches!(claim(&claiming), Ok(Claimed::Ours(_))));
    thread::sleep(Duration::from_millis(100));
    assert!(!claimed.is_finished(), "it waits its turn");
    lock.unlock().unwrap();
    assert!(claimed.join().unwrap());
}

/// A Plays of the test's own at `path`, answering one client with
/// `lines`, as they come, once it asks; then it goes.
fn answering(path: &Path, lines: &'static [&'static str]) -> thread::JoinHandle<()> {
    let Ok(Claimed::Ours(listener)) = claim(path) else {
        panic!("taking the socket")
    };
    thread::spawn(move || {
        let (stream, _) = listener.listener.accept().unwrap();
        let mut asked = String::new();
        BufReader::new(&stream).read_line(&mut asked).unwrap();
        for line in lines {
            writeln!(&stream, "{line}").unwrap();
        }
    })
}

#[test]
fn a_client_passes_over_other_answers_and_keeps_the_events_that_come_first() {
    let path = socket("others");
    let plays = answering(
        &path,
        &[
            r#"{"id": 99, "result": "stale"}"#,
            r#"{"type": "playing", "payload": {"tile_id": "1"}}"#,
            r#"{"id": 1, "result": "hello"}"#,
            r#"{"type": "paused", "payload": null}"#,
        ],
    );
    let mut client = Client::connect(&path).unwrap();
    assert_eq!(client.request(Ask::Hello).unwrap(), "hello");
    assert_eq!(client.events_waiting(), 1);
    plays.join().unwrap();
    assert_eq!(
        client.next_event().unwrap(),
        ("playing".into(), json!({"tile_id": "1"}))
    );
    assert_eq!(client.next_event().unwrap(), ("paused".into(), Value::Null));
    assert_eq!(
        said(client.next_event().unwrap_err()),
        (4, "Plays closed the connection".into()),
        "it went"
    );
}

#[test]
fn an_event_half_read_by_the_time_given_is_read_whole_the_next_time() {
    let path = socket("halves");
    let Ok(Claimed::Ours(listener)) = claim(&path) else {
        panic!("taking the socket")
    };
    let (go_on, told) = mpsc::channel();
    let plays = thread::spawn(move || {
        let (stream, _) = listener.listener.accept().unwrap();
        let mut asked = String::new();
        BufReader::new(&stream).read_line(&mut asked).unwrap();
        writeln!(&stream, r#"{{"id": 1, "result": "hello"}}"#).unwrap();
        write!(&stream, r#"{{"type": "ended", "pay"#).unwrap();
        told.recv().unwrap();
        writeln!(&stream, r#"load": {{"tile_id": "1"}}}}"#).unwrap();
    });
    let mut client = Client::connect(&path).unwrap();
    client.request(Ask::Hello).unwrap();
    let soon = Instant::now() + Duration::from_millis(100);
    assert_eq!(
        client.next_event_by(Some(soon)).unwrap(),
        None,
        "half by then"
    );
    go_on.send(()).unwrap();
    assert_eq!(
        client.next_event_by(None).unwrap(),
        Some(("ended".into(), json!({"tile_id": "1"})))
    );
    plays.join().unwrap();
}

#[test]
fn a_plays_going_without_an_answer_closed_the_connection() {
    let path = socket("gone");
    let plays = answering(&path, &[]);
    let gone = Client::connect(&path).unwrap().request(Ask::Hello);
    plays.join().unwrap();
    assert_eq!(
        said(gone.unwrap_err()),
        (4, "Plays closed the connection".into())
    );
}

#[gpui::test]
fn a_call_comes_back_with_its_result_or_why_not(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("calls", cx);
    let id = window_id(window, &cx);
    let activations = cx.app_activations();

    let (hello, listed, renamed, wrong) = talk(&mut cx, move || {
        let mut client = Client::connect(&path).unwrap();
        let hello = client.request(Ask::Hello).unwrap();
        let listed = client.call("window.ls", Value::Null, Target::default());
        let renamed = client.call("window.rename", json!({"label": "Films"}), in_window("1"));
        let wrong = [
            client.call("window.dance", Value::Null, Target::default()),
            client.call("window.rename", json!({"label": 7}), Target::default()),
            client.call("window.focus", Value::Null, in_window("Attic")),
        ]
        .map(|failed| said(failed.unwrap_err()));
        (hello, listed.unwrap(), renamed.unwrap(), wrong)
    });

    assert_eq!(
        hello,
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "client_id": 1,
        })
    );
    assert_eq!(listed[0]["id"], json!(id));
    assert_eq!(renamed, Value::Null);
    let label = window.read_with(&cx, |workspace, _| workspace.label().map(str::to_owned));
    assert_eq!(label.unwrap().as_deref(), Some("Films"), "it ran");
    let [none, bad, missing] = wrong;
    assert_eq!(none, (1, "no command window.dance".to_owned()));
    assert_eq!(bad.0, 2, "{}", bad.1);
    assert!(bad.1.starts_with("window.rename: "), "{}", bad.1);
    assert_eq!(missing, (3, "no window Attic".to_owned()));
    assert_eq!(
        cx.app_activations(),
        activations,
        "Plays stays behind the terminal"
    );
}

#[gpui::test]
fn with_control_off_it_says_who_it_is_and_takes_files_but_runs_and_tells_nothing(
    cx: &mut TestAppContext,
) {
    let (path, window, mut cx) = serving("control-off", cx);
    allow_control(&mut cx, false);
    assert!(
        !Settings::default().command_line_control,
        "off until Settings turns it on"
    );

    let asking = path.clone();
    let (hello, listed, refused) = talk(&mut cx, move || {
        let mut client = Client::connect(&asking).unwrap();
        let hello = client.request(Ask::Hello).is_ok();
        let listed = client.commands().unwrap().as_array().map_or(0, Vec::len);
        let refused = [
            client
                .call("window.ls", Value::Null, Target::default())
                .unwrap_err(),
            client.subscribe(Heard::All, false).unwrap_err(),
            client
                .call(
                    "settings.set",
                    json!({"key": "command_line_control", "value": true}),
                    Target::default(),
                )
                .unwrap_err(),
        ]
        .map(said);
        (hello, listed, refused)
    });
    assert!(hello, "who it is");
    assert!(listed > 100, "what commands there are: {listed}");
    let off = (5, Error::command_line_off().to_string());
    assert_eq!(refused, [off.clone(), off.clone(), off], "nor turned on");
    let turned_on = TestAppContext::update(&cx, |cx| cx.global::<Settings>().command_line_control);
    assert!(!turned_on);

    let handing = path.clone();
    let handed = talk(&mut cx, move || {
        client::hand_over(&handing, &[fixture("clip.mp4").into()]).map_err(said)
    });
    assert_eq!(handed, Ok(()), "files, as plays FILE hands them over");
    let open = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, _| workspace.players().len() == 1)
            .unwrap()
    });
    assert!(open, "opened");

    allow_control(&mut cx, true);
    let listed = talk(&mut cx, move || {
        let mut client = Client::connect(&path).unwrap();
        client.call("tile.ls", Value::Null, Target::default())
    });
    assert_eq!(
        listed
            .map(|tiles| tiles.as_array().map_or(0, Vec::len))
            .ok(),
        Some(1),
        "once turned on"
    );
}

#[gpui::test]
fn turning_control_off_cuts_off_whoever_is_connected(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("control-cut", cx);
    let (_, hearing) = listening(
        &mut cx,
        &path,
        |client| client.subscribe(Heard::All, false).unwrap(),
        |client| loop {
            if let Err(failure) = client.next_event() {
                let (status, _) = said(failure);
                return vec![("cut off".to_owned(), json!(status))];
            }
        },
    );
    allow_control(&mut cx, false);
    assert!(
        wait_until(&mut cx, |_| hearing.is_finished()),
        "its connection ends"
    );
    assert_eq!(
        hearing.join().unwrap(),
        [("cut off".to_owned(), json!(4))],
        "as if Plays went"
    );
}

#[gpui::test]
fn commands_are_listed_and_described(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("listed", cx);
    let (commands, described, none) = talk(&mut cx, move || {
        let mut client = Client::connect(&path).unwrap();
        let describe = |client: &mut Client, command: &str| {
            client.request(Ask::Describe {
                command: command.into(),
            })
        };
        (
            client.request(Ask::Commands).unwrap(),
            describe(&mut client, "window.ls").unwrap(),
            said(describe(&mut client, "window.dance").unwrap_err()),
        )
    });
    let all: Vec<Value> = api::all().map(api::Spec::describe).collect();
    assert_eq!(commands, Value::from(all));
    assert_eq!(described, api::find("window.ls").unwrap().describe());
    assert_eq!(none, (1, "no command window.dance".to_owned()));
}

#[gpui::test]
fn a_request_it_cant_read_is_answered_so_and_the_client_goes_on(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("unread", cx);
    let answers = talk(&mut cx, move || {
        let stream = UnixStream::connect(&path).unwrap();
        let mut lines = BufReader::new(stream.try_clone().unwrap()).lines();
        let mut ask = |line: &str| {
            writeln!(&stream, "{line}").unwrap();
            let answer = lines.next().expect("an answer").unwrap();
            serde_json::from_str::<Value>(&answer).unwrap()
        };
        [
            ask("nonsense"),
            ask(r#"{"id": 7, "method": "dance"}"#),
            // Blank lines aren't asking anything.
            ask("\n \r\n{\"id\": 8, \"method\": \"hello\"}"),
        ]
    });
    let [nonsense, dance, hello] = answers;
    assert_eq!(nonsense["id"], Value::Null);
    assert_eq!(nonsense["error"], "bad_request");
    assert_eq!(dance["id"], 7);
    assert_eq!(dance["error"], "bad_request");
    assert!(
        dance["message"].as_str().unwrap().contains("dance"),
        "{dance}"
    );
    assert_eq!(hello["id"], 8);
    assert_eq!(hello["result"]["client_id"], 1);
}

#[gpui::test]
fn a_client_sending_too_long_a_line_is_cut_off(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("long", cx);
    let sending = path.clone();
    let answer = talk(&mut cx, move || {
        let mut stream = UnixStream::connect(&sending).unwrap();
        let line = vec![b'{'; usize::try_from(LONGEST).unwrap() + 1];
        // It may be cut off before all of it is written.
        stream.write_all(&line).ok();
        let mut answer = String::new();
        BufReader::new(&stream).read_line(&mut answer).ok();
        answer
    });
    assert_eq!(answer, "", "it's cut off without an answer");

    let hello = talk(&mut cx, move || {
        Client::connect(&path).unwrap().request(Ask::Hello).unwrap()
    });
    assert_eq!(hello["client_id"], 2, "others are answered");
}

#[gpui::test]
fn a_plays_started_with_files_hands_them_to_the_one_running(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("handover", cx);
    let activations = cx.app_activations();
    let handed = talk(&mut cx, move || {
        let relative = OsString::from("tests/fixtures/clip.mp4");
        client::hand_over(&path, &[relative]).map_err(said)
    });
    assert_eq!(handed, Ok(()));

    let clip = fixture("clip.mp4").display().to_string();
    let open = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                let files = workspace.players();
                files.len() == 1 && files[0].read(cx).summary()["file"] == clip
            })
            .unwrap()
    });
    assert!(open, "it opens in the window in front, by its full path");
    assert_eq!(
        cx.app_activations(),
        activations + 1,
        "Plays comes to the front"
    );
}

/// Started as another process by the test after it, as `plays ARGS`
/// would be: hands what `PLAYS_HAND_OVER` lists, a line each, to the Plays
/// answering at `PLAYS_SOCKET`, and exits with the status, as `main` does.
/// Passes by itself.
#[test]
#[ignore = "started as another process by the test after it"]
fn hand_over_as_another_process() {
    let (Some(path), Some(args)) = (
        std::env::var_os("PLAYS_SOCKET"),
        std::env::var("PLAYS_HAND_OVER").ok(),
    ) else {
        return;
    };
    let args: Vec<OsString> = args.lines().map(OsString::from).collect();
    match start_at(Some(path.into()), &args) {
        Start::HandedOver(status) => std::process::exit(status.into()),
        Start::Runs(_) => std::process::exit(99),
    }
}

/// How another Plays started as `plays ARGS`, with the one at `path`
/// running, exits: as this test binary, running only
/// `hand_over_as_another_process`.
fn another_plays(cx: &mut TestAppContext, path: &Path, args: &[&str]) -> Option<i32> {
    let mut plays = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "api::socket::tests::hand_over_as_another_process",
        ])
        .env("PLAYS_SOCKET", path)
        .env("PLAYS_HAND_OVER", args.join("\n"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut exited = None;
    let done = wait_until(cx, |_| {
        exited = plays.try_wait().unwrap();
        exited.is_some()
    });
    assert!(done, "the other Plays exited");
    exited.unwrap().code()
}

#[gpui::test]
fn another_plays_started_with_files_hands_them_over_and_exits(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("process", cx);
    assert_eq!(
        another_plays(&mut cx, &path, &["tests/fixtures/clip.mp4"]),
        Some(0)
    );
    let clip = fixture("clip.mp4").display().to_string();
    let open = wait_until(&mut cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                let files = workspace.players();
                files.len() == 1 && files[0].read(cx).summary()["file"] == clip
            })
            .unwrap()
    });
    assert!(open, "the file opens in this one");

    let refused = another_plays(&mut cx, &path, &["gopher://192.0.2.1/a.mkv"]);
    assert_eq!(refused, Some(2), "with the status for what it can't open");
}

#[test]
fn a_plays_starting_with_none_running_takes_the_socket() {
    let path = socket("first");
    let Start::Runs(Some(_listener)) = start_at(Some(path.clone()), &[]) else {
        panic!("taking the socket")
    };
    assert!(UnixStream::connect(&path).is_ok());
    let in_the_way = socket("in-the-way");
    fs::create_dir_all(&in_the_way).unwrap();
    assert!(
        matches!(start_at(Some(in_the_way), &[]), Start::Runs(None)),
        "with something else there, it runs without one"
    );
}

#[gpui::test]
fn files_are_handed_over_by_their_full_paths_and_streams_as_given(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("addresses", cx);
    TestAppContext::update(&cx, api::intercept);
    let handed = talk(&mut cx, move || {
        let args = [
            "tests/fixtures/clip.mp4",
            "/media/film.mkv",
            "https://192.0.2.1/live.m3u8",
        ]
        .map(OsString::from);
        client::hand_over(&path, &args).map_err(said)
    });
    assert_eq!(handed, Ok(()));
    let urls = json!([
        fixture("clip.mp4").display().to_string(),
        "/media/film.mkv",
        "https://192.0.2.1/live.m3u8",
    ]);
    assert_eq!(
        TestAppContext::update(&cx, api::recorded),
        [Invocation::new("app.open")
            .with_args(json!({"urls": urls}))
            .from(api::Source::Cli)]
    );
}

#[gpui::test]
fn a_plays_started_with_nothing_brings_the_one_running_forward(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("forward", cx);
    let id = window_id(window, &cx);
    let activations = cx.app_activations();
    TestAppContext::update(&cx, api::record);
    let handing = path.clone();
    let handed = talk(&mut cx, move || {
        client::hand_over(&handing, &[]).map_err(said)
    });
    assert_eq!(handed, Ok(()));
    assert_eq!(cx.app_activations(), activations + 1);
    assert_eq!(
        TestAppContext::update(&cx, api::recorded),
        [Invocation::new("window.focus")
            .in_window(id)
            .from(api::Source::Cli)]
    );

    // With no window open, it opens one.
    window
        .update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    let handed = talk(&mut cx, move || client::hand_over(&path, &[]).map_err(said));
    assert_eq!(handed, Ok(()));
    assert_eq!(
        TestAppContext::update(&cx, api::recorded),
        [Invocation::new("window.new").from(api::Source::Cli)]
    );
    assert!(TestAppContext::update(&cx, |cx| api::front_window(cx)).is_some());
}

#[gpui::test]
fn a_plays_that_cant_open_what_its_handed_says_why(cx: &mut TestAppContext) {
    let (path, window, mut cx) = serving("refused", cx);
    let handed = talk(&mut cx, move || {
        client::hand_over(&path, &[OsString::from("gopher://192.0.2.1/a.mkv")]).map_err(said)
    });
    let (status, why) = handed.unwrap_err();
    assert_eq!(status, 2);
    assert!(why.contains("gopher:// streams aren't supported"), "{why}");
    let files = window.read_with(&cx, |workspace, _| workspace.players().len());
    assert_eq!(files.unwrap(), 0);
}

#[gpui::test]
fn events_reach_those_who_subscribe_as_they_happen(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("events", cx);
    let (_, everything) = listening(
        &mut cx,
        &path,
        |client| client.subscribe(Heard::All, false).unwrap(),
        |client| until(client, "client_left"),
    );
    let (_, apps) = listening(
        &mut cx,
        &path,
        |client| client.subscribe(Heard::App, false).unwrap(),
        |client| until(client, "client_left"),
    );

    let clip = fixture("clip.mp4").display().to_string();
    talk(&mut cx, move || {
        let mut client = Client::connect(&path).unwrap();
        let open = json!({"urls": [clip]});
        client.call("app.open", open, Target::default()).unwrap();
        // Its tile is made in a moment, and seen as the next command runs.
        let tiles = |client: &mut Client| {
            let listed = client.call("tile.ls", Value::Null, Target::default());
            listed.unwrap().as_array().map_or(0, Vec::len)
        };
        while tiles(&mut client) == 0 {
            thread::sleep(Duration::from_millis(5));
        }
        let rename = json!({"label": "Films"});
        client
            .call("window.rename", rename, Target::default())
            .unwrap();
    });
    assert!(wait_until(&mut cx, |_| everything.is_finished() && apps.is_finished()));

    // The second subscriber connected as the first listened.
    let everything = everything.join().unwrap();
    let apps = apps.join().unwrap();
    assert_eq!(
        everything[0],
        ("client_connected".into(), json!({"client_id": 2}))
    );
    let everything = &everything[1..];
    assert_eq!(
        kinds(&apps),
        [
            "client_connected",
            "command_run",
            "command_run",
            "window_label_changed",
            "client_left",
        ]
    );
    assert_eq!(apps[0].1, json!({"client_id": 3}));
    assert_eq!(apps[1].1["command"], "app.open");
    assert_eq!(apps[1].1["source"], "cli", "from the command line");
    assert_eq!(apps[2].1["command"], "window.rename");
    assert_eq!(apps[3].1["label"], "Films");
    assert_eq!(apps[4].1, json!({"client_id": 3}));
    assert!(
        kinds(everything).contains(&"tile_created"),
        "what's in a window is heard by those listening to everything: {:?}",
        kinds(everything)
    );
    let heard_by_both: Vec<&(String, Value)> = everything
        .iter()
        .filter(|(kind, _)| kinds(&apps).contains(&kind.as_str()))
        .collect();
    assert_eq!(heard_by_both, apps.iter().collect::<Vec<_>>());
}

#[gpui::test]
fn a_subscriber_to_one_window_hears_only_its_events(cx: &mut TestAppContext) {
    let (path, first, mut cx) = serving("window", cx);
    let second = TestAppContext::update(&cx, |cx| Workspace::open_window(WINDOW_SIZE, cx)).unwrap();
    cx.run_until_parked();
    let (answer, hearing) = listening(
        &mut cx,
        &path,
        |client| client.subscribe(Heard::Window("1".into()), false).unwrap(),
        |client| vec![client.next_event().unwrap()],
    );
    assert_eq!(answer, json!({"window": window_id(first, &cx)}));

    let renaming = path.clone();
    talk(&mut cx, move || {
        let mut client = Client::connect(&renaming).unwrap();
        for (window, label) in [("2", "Two"), ("1", "One")] {
            let rename = json!({"label": label});
            client
                .call("window.rename", rename, in_window(window))
                .unwrap();
        }
    });
    assert!(wait_until(&mut cx, |_| hearing.is_finished()));
    assert_eq!(
        hearing.join().unwrap(),
        [(
            "window_label_changed".into(),
            json!({"window_id": window_id(first, &cx), "window_title": "One", "label": "One"})
        )]
    );
    assert_ne!(window_id(second, &cx), window_id(first, &cx));

    let none = talk(&mut cx, move || {
        let mut client = Client::connect(&path).unwrap();
        said(
            client
                .subscribe(Heard::Window("Attic".into()), false)
                .unwrap_err(),
        )
    });
    assert_eq!(none, (3, "no window Attic".to_owned()));
}

#[gpui::test]
fn positions_reach_only_those_who_ask_for_them(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("positions", cx);
    let (_, hearing) = listening(
        &mut cx,
        &path,
        |client| client.subscribe(Heard::All, true).unwrap(),
        |client| until(client, "position"),
    );
    let (_, without) = listening(
        &mut cx,
        &path,
        |client| client.subscribe(Heard::All, false).unwrap(),
        |client| until(client, "window_label_changed"),
    );
    let clip = fixture("clip.mp4").display().to_string();
    let opened = TestAppContext::update(&cx, |cx| {
        api::run(
            Invocation::new("app.open").with_args(json!({"urls": [clip]})),
            cx,
        )
    });
    assert!(opened.is_ok(), "{opened:?}");
    let mut heard = false;
    for _ in 0..100 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        if hearing.is_finished() {
            heard = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(heard, "where the file is was told");
    let position = hearing.join().unwrap().pop().unwrap();
    assert!(position.1["position"].is_number(), "{position:?}");

    // The other hears none before what comes after.
    let renamed = TestAppContext::update(&cx, |cx| {
        api::run(
            Invocation::new("window.rename").with_args(json!({"label": "Films"})),
            cx,
        )
    });
    assert_eq!(renamed, Ok(Value::Null));
    assert!(wait_until(&mut cx, |_| without.is_finished()));
    let without = without.join().unwrap();
    assert!(
        !kinds(&without).contains(&"position"),
        "{:?}",
        kinds(&without)
    );
}

#[gpui::test]
fn a_subscriber_running_a_command_hears_it_run_before_the_answer(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("own", cx);
    let (waiting, first) = talk(&mut cx, move || {
        let mut client = Client::connect(&path).unwrap();
        client.subscribe(Heard::All, false).unwrap();
        let rename = json!({"label": "Films"});
        client
            .call("window.rename", rename, Target::default())
            .unwrap();
        (client.events_waiting(), client.next_event().unwrap())
    });
    assert_eq!(waiting, 1);
    assert_eq!(first.0, "command_run");
    assert_eq!(first.1["command"], "window.rename");
}

#[gpui::test]
fn after_unsubscribing_nothing_more_is_heard(cx: &mut TestAppContext) {
    let (path, _, mut cx) = serving("unheard", cx);
    let waiting = talk(&mut cx, move || {
        let mut client = Client::connect(&path).unwrap();
        client.subscribe(Heard::All, false).unwrap();
        client.request(Ask::Unsubscribe).unwrap();
        let rename = json!({"label": "Films"});
        client
            .call("window.rename", rename, Target::default())
            .unwrap();
        client.request(Ask::Hello).unwrap();
        client.events_waiting()
    });
    assert_eq!(waiting, 0);
}

#[gpui::test]
fn the_socket_is_removed_as_the_app_quits(cx: &mut TestAppContext) {
    let (path, _, cx) = serving("quits", cx);
    assert!(path.exists());
    cx.quit();
    assert!(!path.exists());
}

/// What a client hears until `quit`, which its connection closes after.
fn to_the_end(client: &mut Client) -> Vec<(String, Value)> {
    let heard = until(client, "quit");
    assert_eq!(
        client.next_event().map_err(said),
        Err((4, "Plays closed the connection".into())),
        "then the end"
    );
    heard
}

#[gpui::test]
fn as_the_app_quits_listeners_hear_the_windows_close_and_quit_then_the_end(
    cx: &mut TestAppContext,
) {
    let (path, window, mut cx) = serving("quitting", cx);
    let w = window_id(window, &cx);
    let (_, all) = listening(
        &mut cx,
        &path,
        |client| client.subscribe(Heard::All, false).unwrap(),
        to_the_end,
    );
    let (_, first) = listening(
        &mut cx,
        &path,
        |client| client.subscribe(Heard::Window("1".into()), false).unwrap(),
        to_the_end,
    );
    // One that only asked: its connection closes too.
    let (_, asked) = listening(
        &mut cx,
        &path,
        |client| client.request(Ask::Hello).unwrap(),
        |client| {
            let end = client.next_event().map_err(said);
            vec![("end".into(), json!(end.unwrap_err().1))]
        },
    );

    cx.quit();
    let all = all.join().expect("all heard to the end");
    assert_eq!(
        kinds(&all),
        [
            "client_connected",
            "client_connected",
            "window_closed",
            "active_window_changed",
            "quit",
        ]
    );
    assert_eq!(all[2].1, json!({"window_id": w, "window_title": "Plays"}));
    assert_eq!(all[4].1, json!({}));
    let first = first.join().expect("the window's heard to the end");
    assert_eq!(kinds(&first), ["window_closed", "quit"]);
    assert_eq!(
        asked.join().unwrap(),
        [("end".into(), json!("Plays closed the connection"))]
    );
    assert!(!path.exists());
}

#[test]
fn a_command_with_plays_running_reaches_it_without_starting_another() {
    let path = socket("reached");
    let Ok(Claimed::Ours(_listener)) = claim(&path) else {
        panic!("taking the socket")
    };
    let start: &dyn Fn() -> io::Result<()> = &|| panic!("started another");
    assert!(client::connect_or_start(&path, Some(start)).is_ok());
}

#[test]
fn a_command_with_no_plays_running_starts_it_and_waits_for_it() {
    let path = socket("autostart");
    let started = AtomicUsize::new(0);
    let starting = path.clone();
    let start: &dyn Fn() -> io::Result<()> = &|| {
        started.fetch_add(1, Ordering::SeqCst);
        let path = starting.clone();
        // It takes a moment to start.
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            let Ok(Claimed::Ours(listener)) = claim(&path) else {
                panic!("taking the socket")
            };
            listener.listener.accept().ok();
        });
        Ok(())
    };
    assert!(client::connect_or_start(&path, Some(start)).is_ok());
    assert_eq!(started.load(Ordering::SeqCst), 1);
}

#[test]
fn a_command_with_no_plays_running_and_none_to_start_says_so() {
    let path = socket("unreached");
    let none = client::connect_or_start(&path, None)
        .map(drop)
        .map_err(said);
    let (status, why) = none.unwrap_err();
    assert_eq!(status, 4);
    assert!(why.starts_with("can't reach Plays: "), "{why}");

    let failing: &dyn Fn() -> io::Result<()> = &|| Err(io::Error::other("no such program"));
    let failed = client::connect_or_start(&path, Some(failing)).map(drop);
    assert_eq!(
        failed.map_err(said),
        Err((4, "can't reach Plays: no such program".to_owned()))
    );
}

#[test]
fn a_plays_that_cant_be_reached_for_another_reason_isnt_started() {
    let path = socket("denied");
    let folder = path.parent().unwrap();
    fs::create_dir_all(folder).unwrap();
    fs::set_permissions(folder, fs::Permissions::from_mode(0o000)).unwrap();
    let start: &dyn Fn() -> io::Result<()> = &|| panic!("started another");
    let denied = client::connect_or_start(&path, Some(start)).map(drop);
    fs::set_permissions(folder, fs::Permissions::from_mode(0o700)).unwrap();
    let (status, why) = denied.map_err(said).unwrap_err();
    assert_eq!(status, 4);
    assert!(why.contains("ermission denied"), "{why}");
}

#[test]
fn each_failure_has_its_exit_status() {
    let cases = [
        (Failure::Unreachable(io::ErrorKind::NotFound.into()), 4),
        (Failure::Closed, 4),
        (Failure::Command(Error::failed("no")), 1),
        (Failure::Command(Error::bad_args("a.b", "no")), 2),
        (Failure::Command(Error::not_found("tile", "x")), 3),
        (Failure::BadRequest("no".into()), 1),
    ];
    for (failure, status) in cases {
        assert_eq!(failure.exit_status(), status, "{failure}");
    }
    assert_eq!(
        Failure::BadRequest("no method".into()).to_string(),
        "Plays didn't understand: no method"
    );
}
