//! Settings → Default Player, over a stand-in for the system's defaults:
//! nothing on this computer is read or changed.

use std::cell::RefCell;

use file_types::{Format, format_of};
use gpui::{Task, TestAppContext, VisualTestContext};

use super::open_settings;
use crate::api::{self, Invocation};
use crate::app::default_player::{Made, Opener, System};
use crate::ui::testing::{self, click, painted};

thread_local! {
    /// The stand-in's formats and the app each opens in.
    static APPS: RefCell<Vec<Opener>> = const { RefCell::new(Vec::new()) };
    /// The formats it was asked to make Plays open, each time.
    static ASKED: RefCell<Vec<Vec<&'static str>>> = const { RefCell::new(Vec::new()) };
    /// Why Plays can't be the default player, if it can't.
    static CANT: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn format(extension: &str) -> &'static Format {
    format_of(extension).unwrap()
}

/// A system where Matroska and FLAC open in VLC, MP3 in VLC too, which it
/// keeps when asked (as Keep on macOS does), MPEG-4 and AAC in Plays
/// already, and WebM in nothing.
fn stand_in(cx: &mut VisualTestContext) {
    let opener = |extension, app: Option<&str>| Opener {
        format: format(extension),
        app: app.map(str::to_owned),
        plays: app == Some("Plays"),
    };
    APPS.set(vec![
        opener("mkv", Some("VLC")),
        opener("mp4", Some("Plays")),
        opener("webm", None),
        opener("aac", Some("Plays")),
        opener("mp3", Some("VLC")),
        opener("flac", Some("VLC")),
    ]);
    let system = System {
        cant: || CANT.with_borrow(Clone::clone),
        read: |_| Task::ready(Ok(APPS.with_borrow(Clone::clone))),
        make: |formats, _| {
            ASKED.with_borrow_mut(|asked| {
                asked.push(formats.iter().map(|format| format.extension()).collect());
            });
            let mut made = Made::default();
            for format in formats {
                if format.extension() == "mp3" {
                    made.not_set
                        .push((format, "VLC still opens them".to_owned()));
                    continue;
                }
                APPS.with_borrow_mut(|apps| {
                    let opener = apps.iter_mut().find(|o| o.format == format).unwrap();
                    opener.app = Some("Plays".to_owned());
                    opener.plays = true;
                });
                made.set.push(format);
            }
            Task::ready(made)
        },
    };
    cx.update(|_, cx| cx.set_global(system));
}

/// The text drawn, its lines joined by spaces.
fn text(cx: &mut VisualTestContext) -> String {
    let lines: Vec<String> = painted(cx)
        .into_iter()
        .map(|(line, _)| line.to_string())
        .collect();
    lines.join(" ")
}

/// Open Settings at Default Player as its command does.
fn open_default_player(cx: &mut VisualTestContext) -> VisualTestContext {
    cx.update(|_, cx| api::dispatch(Invocation::new("app.default_player"), cx));
    cx.run_until_parked();
    open_settings(cx)
}

#[gpui::test]
fn the_formats_picked_and_only_they_are_asked_for(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    stand_in(&mut cx);
    let mut window = open_default_player(&mut cx);

    let shown = text(&mut window);
    assert!(shown.contains("Open files with Plays"), "{shown}");
    assert!(shown.contains("Matroska video"), "{shown}");
    assert!(shown.contains(".mkv · VLC opens them"), "{shown}");
    assert!(shown.contains(".mp4 · Plays opens them"), "{shown}");
    assert!(shown.contains(".webm · No app opens them"), "{shown}");

    // Nothing picked, nothing asked; MPEG-4 is Plays' already, for good.
    click(&mut window, "default-player-use");
    click(&mut window, "default-player-mp4");
    click(&mut window, "default-player-use");
    assert!(ASKED.with_borrow(Vec::is_empty));

    click(&mut window, "default-player-mkv");
    click(&mut window, "default-player-all-audio");
    assert!(text(&mut window).contains("Use Plays for 3"));
    click(&mut window, "default-player-use");
    window.run_until_parked();
    assert_eq!(ASKED.with_borrow(Clone::clone), [["mkv", "mp3", "flac"]]);

    let shown = text(&mut window);
    assert!(
        shown.contains(
            "Plays opens 2 of the 3 formats picked now. Not MP3 audio (.mp3): VLC still opens them."
        ),
        "{shown}"
    );
    assert!(
        shown.contains(".mkv · Plays opens them"),
        "read again: {shown}"
    );
    assert!(shown.contains(".mp3 · VLC opens them"), "{shown}");
    assert!(
        !shown.contains("Use Plays for"),
        "nothing picked now: {shown}"
    );
}

#[gpui::test]
fn default_player_says_why_plays_cant_be_it(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    stand_in(&mut cx);
    CANT.set(Some(
        "only Plays.app can be the default player, and this Plays isn't it".to_owned(),
    ));
    let mut window = open_default_player(&mut cx);

    let shown = text(&mut window);
    assert!(
        shown.contains("Only Plays.app can be the default player"),
        "{shown}"
    );
    assert!(
        shown.contains(".mkv · VLC opens them"),
        "still listed: {shown}"
    );
    click(&mut window, "default-player-mkv");
    click(&mut window, "default-player-all-video");
    assert!(
        !text(&mut window).contains("Use Plays for"),
        "nothing to pick"
    );
    click(&mut window, "default-player-use");
    assert!(ASKED.with_borrow(Vec::is_empty), "nothing asked");
}

#[gpui::test]
fn default_player_reads_again_each_time_it_shows(cx: &mut TestAppContext) {
    let (_, mut cx) = testing::start(cx);
    stand_in(&mut cx);
    let mut window = open_default_player(&mut cx);
    click(&mut window, "default-player-mkv");
    assert!(text(&mut window).contains(".mkv · VLC opens them"));

    // Matroska is mpv's now, as Finder could have made it meanwhile.
    click(&mut window, "pane-Playback");
    APPS.with_borrow_mut(|apps| apps[0].app = Some("mpv".to_owned()));
    click(&mut window, "pane-Default Player");
    let shown = text(&mut window);
    assert!(shown.contains(".mkv · mpv opens them"), "{shown}");
    click(&mut window, "default-player-use");
    assert!(
        ASKED.with_borrow(Vec::is_empty),
        "what was picked is forgotten"
    );
}
