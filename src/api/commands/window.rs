//! `window`: player windows: opening one, or files into one, naming one,
//! going from one to another and back, numbering them, closing one or the
//! others, full screen and the display it's on, and floating them on top.

use gpui::{AnyWindowHandle, App};
use serde::Deserialize;
use serde_json::{Value, json};

use super::done;
use crate::api::args::{Arg, Kind};
use crate::api::registry::{Args, Outcome, Reach, Scope, Spec, on_mac};
use crate::api::target::{self, Target};
use crate::api::{Error, Invocation, Run, Source};
use crate::app::{back_window, move_window, player_windows, recent_window, step_window};
use crate::recents;
use crate::settings::{FloatOnTop, Settings};
use crate::ui::palette::{self, Listing};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

pub const COMMANDS: &[Spec] = &[
    Spec::query(
        "window.ls",
        "List Windows",
        "List the player windows, in the order ⌘1 to ⌘9 number them.",
        Scope::App,
        "[{id, label, title, number, front, tiles}]: each window's id, label (or null) \
         and title, as its title bar shows it, its number from 1, whether it's in front, \
         and how many tiles it has.",
        |_, _, cx| Ok(Value::from(summaries(cx)?)),
    )
    .keywords(&["list", "ls", "windows"]),
    Spec::query(
        "window.inspect",
        "Inspect Window",
        "Say all about the window: where it is, how it shows, its output and its tiles.",
        Scope::Window,
        "{id, label, title, number, front, tiles: [as tile.ls], bounds: {x, y, width, height}, \
         display, full_screen, on_top, output: {id, name} or null}: as window.ls says, \
         its tiles as tile.ls lists them, where it is in points, the id of the display \
         it's on, whether it's full screen and floats on top, and its own output.",
        inspect,
    )
    .keywords(&["info", "details", "describe"]),
    Spec::app_with(
        "window.new",
        "New Window",
        "Open an empty player window, in front, numbered after the others.",
        &[],
        new_window,
    )
    .keywords(&["open", "empty", "another", "create"])
    .keys(&["secondary-n"])
    .returns("{window}: the new window's id."),
    Spec::window(
        "window.open_files",
        "Open…",
        "Choose files or folders to play in the window.",
        |workspace, window, cx| {
            window.activate_window();
            workspace.prompt_open(window, cx);
        },
    )
    .keywords(&["file", "folder", "add", "play"])
    .keys(&["secondary-o"])
    // An empty window has no files for keys to need.
    .reach(Reach::Anywhere),
    Spec::window(
        "window.open_url",
        "Open URL…",
        "Type the URL of a stream to play in the window.",
        |workspace, window, cx| {
            window.activate_window();
            workspace.show_open_url(window, cx);
        },
    )
    .keywords(&["stream", "link", "network", "http", "rtsp"])
    .keys(&["secondary-shift-o"])
    .reach(Reach::Anywhere),
    Spec::custom(
        palette::COMMAND,
        "Command Palette…",
        "List every command that can run in the window, to find one by name and run it, \
         picking or typing what it takes.",
        Scope::Window,
        show_palette,
    )
    .keywords(&["commands", "actions", "run", "search", "find"])
    .keys(&["secondary-shift-p"])
    // An empty window has commands to run too.
    .reach(Reach::Anywhere),
    Spec::custom(
        palette::GO_TO,
        "Go to…",
        "List the windows, their tiles, the files in the front tile's folder and those \
         played lately, to find one by name and go there: bring it to the front, play \
         the file in the tile, or open it.",
        Scope::Window,
        show_go_to,
    )
    .keywords(&[
        "switch",
        "find",
        "files",
        "recent",
        "windows",
        "tiles",
        "quick open",
    ])
    .keys(&["secondary-p"])
    // An empty window has places to go to too.
    .reach(Reach::Anywhere),
    Spec::custom(
        "window.close",
        "Close Window",
        "Close the window and the files in it; with none named, the window in front, \
         Settings and About too.",
        Scope::AnyWindow,
        close,
    )
    // ⌘W closes the file in front, and this where there's none.
    .keys(&["secondary-alt-w", "secondary-w"]),
    Spec::custom(
        "window.close_others",
        "Close Other Windows",
        "Close every player window but this one, and the files in them.",
        Scope::Window,
        close_others,
    )
    .keywords(&["close all", "others", "keep"]),
    Spec::window_with(
        "window.rename",
        "Rename Window…",
        "Label the window: its title shows the label, and it can be named by it. A blank \
         one takes the label away; with none given, the window asks for one.",
        &[Arg::optional(
            "label",
            Kind::Text,
            "What to call it; no two windows are called the same.",
        )],
        |args, workspace, window, cx| {
            let Rename { label } = args.read()?;
            match label {
                Some(label) => workspace
                    .set_label(&label, window, cx)
                    .map_err(|why| Error::bad_args(args.command, why))?,
                None => {
                    window.activate_window();
                    workspace.ask_for_label(window, cx);
                }
            }
            done()
        },
    )
    .keywords(&["name", "label", "title"]),
    Spec::window(
        "window.focus",
        "Go to Window",
        "Bring the window to the front. ⌘1 to ⌘9 go to the first nine, by their \
         numbers.",
        |_, window, _| window.activate_window(),
    )
    .keywords(&["switch", "show", "raise"])
    // The window in front is in front already; ⌘1 to ⌘9 name theirs.
    .needs_target(),
    Spec::app(
        "window.next",
        "Next Window",
        "Bring the window after the one in front to the front: the first after the last.",
        |cx| step_window(1, cx),
    )
    .keywords(&["switch", "cycle"])
    .keys(&["ctrl-tab"]),
    Spec::app(
        "window.previous",
        "Previous Window",
        "Bring the window before the one in front to the front: the last before the first.",
        |cx| step_window(-1, cx),
    )
    .keywords(&["switch", "cycle"])
    .keys(&["ctrl-shift-tab"]),
    Spec::app(
        "window.back",
        "Go Back One Window",
        "Bring back the player window in front before this one; again, the one before \
         that, and so on, until something else is done.",
        back_window,
    )
    .keywords(&["history", "previous", "last", "return"]),
    Spec::app(
        "window.recent",
        "Go to Recent Window",
        "Bring back the player window in front last; again, the one in front first.",
        recent_window,
    )
    .keywords(&["history", "last", "toggle", "alternate", "switch"])
    .keys(&["secondary-'"]),
    Spec::custom_with(
        "window.move",
        "Move Window To",
        "Give the window another number, from 1, which ⌘1 to ⌘9 go to it by: the windows \
         between move up or down one.",
        Scope::Window,
        &[Arg::required(
            "position",
            Kind::Integer { min: 1 },
            "Its number.",
        )],
        move_to_number,
    )
    .keywords(&["reorder", "number", "renumber", "order"])
    .state(number),
    Spec::custom(
        "window.move.backward",
        "Move Window Backward",
        "Give the window the number before its own, the window there taking its own.",
        Scope::Window,
        |_, target, cx| step_number(target, -1, cx),
    )
    .keywords(&["reorder", "number", "earlier"]),
    Spec::custom(
        "window.move.forward",
        "Move Window Forward",
        "Give the window the number after its own, the window there taking its own.",
        Scope::Window,
        |_, target, cx| step_number(target, 1, cx),
    )
    .keywords(&["reorder", "number", "later"]),
    Spec::window(
        "window.toggle_full_screen",
        "Full Screen",
        "Show the window full screen, or as it was again.",
        |_, window, _| window.toggle_fullscreen(),
    )
    .keywords(&["enter", "exit", "maximize"])
    .setting("full_screen")
    .keys(on_mac(&["ctrl-cmd-f", "f"], &["f11", "f"]))
    .state(|target, cx| {
        target::in_window(target, cx, |_, window, _| Ok(json!(window.is_fullscreen())))
    }),
    Spec::window(
        "window.exit_full_screen",
        "Exit Full Screen",
        "Show the window as it was before it was full screen.",
        |_, window, _| {
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        },
    )
    .setting("exit_full_screen")
    .keys(&["escape"]),
    Spec::window(
        "window.match_video_size",
        "Match Video Size",
        "Size the window for its videos to show at their own size, as far as the screen \
         allows.",
        |workspace, window, cx| workspace.match_video_size(window, cx),
    )
    .keywords(&["fit", "resize", "actual size"])
    .setting("match_video_size")
    .keys(&["secondary-alt-0"]),
    Spec::app(
        "window.toggle_float_on_top",
        "Float on Top",
        "Keep player windows above other apps' windows, or stop.",
        |cx| toggle_float_on_top(FloatOnTop::Always, cx),
    )
    .keywords(&["pin", "always on top", "above"])
    .setting("float_on_top")
    .state(|_, cx| Ok(json!(floats(FloatOnTop::Always, cx)))),
    Spec::app(
        "window.toggle_float_on_top_while_playing",
        "Float on Top While Playing",
        "Keep player windows above other apps' windows while a file in them plays, or stop.",
        |cx| toggle_float_on_top(FloatOnTop::WhilePlaying, cx),
    )
    .keywords(&["pin", "always on top", "above"])
    .setting("float_on_top_while_playing")
    .state(|_, cx| Ok(json!(floats(FloatOnTop::WhilePlaying, cx)))),
    Spec::window_with(
        "window.show_on_display",
        "Show on Display",
        "Show the window full screen on a display, coming back where it was once it \
         leaves full screen there.",
        &[Arg::required(
            "display",
            Kind::Integer { min: 0 },
            "The display's id.",
        )],
        |args, workspace, window, cx| {
            let OnDisplay { display } = args.read()?;
            workspace.show_on_display(display, window, cx);
            done()
        },
    )
    .keywords(&["screen", "monitor", "projector", "move"]),
];

/// Show the command palette over the window, listing the commands that
/// can run there as they are now, and run lately.
fn show_palette(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let here = here(target, cx)?;
    let listing = Listing::Commands {
        entries: palette::entries(&here, cx),
        recents: recents::COMMANDS.list(cx),
    };
    show(listing, &here, cx)
}

/// Show Go to… over the window, listing the windows, the tiles, the files
/// in the front tile's folder and those played lately.
fn show_go_to(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let here = here(target, cx)?;
    let listing = Listing::Places(palette::places(&here, cx));
    show(listing, &here, cx)
}

/// The target naming the window `target` names, or the one in front.
fn here(target: &Target, cx: &App) -> Result<Target, Error> {
    let window = target::window(target, cx)?;
    Ok(Target {
        window: Some(target::read(window, cx)?.id().to_string()),
        tile: None,
    })
}

/// Show the palette listing `listing` over the window `here` names, and
/// bring it to the front.
fn show(listing: Listing, here: &Target, cx: &mut App) -> Outcome {
    target::in_window(here, cx, |workspace, window, cx| {
        window.activate_window();
        workspace.show_palette(listing, window, cx);
        done()
    })
}

#[derive(Deserialize)]
struct OnDisplay {
    display: u32,
}

#[derive(Deserialize)]
struct Rename {
    #[serde(default)]
    label: Option<String>,
}

#[derive(Deserialize)]
struct Position {
    position: usize,
}

/// What `window.ls` says of each player window, in order.
pub fn summaries(cx: &App) -> Result<Vec<Value>, Error> {
    let front = target::front_window(cx);
    target::windows(cx)
        .into_iter()
        .enumerate()
        .map(|(at, window)| {
            let summary = target::read(window, cx)?.summary(at + 1, front == Some(window));
            Ok(Value::Object(summary))
        })
        .collect()
}

/// All about the window named, or the one in front.
fn inspect(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let (window, at, _) = place(target, cx)?;
    let front =
        target::front_window(cx).is_some_and(|front| AnyWindowHandle::from(front) == window);
    target::in_window(target, cx, |workspace, window, cx| {
        Ok(Value::Object(workspace.inspect(at + 1, front, window, cx)))
    })
}

/// Open an empty player window, in front.
fn new_window(_: Args, cx: &mut App) -> Outcome {
    let window = Workspace::open_window(WINDOW_SIZE, cx)
        .map_err(|err| Error::failed(format!("can't open a window: {err:#}")))?;
    let id = window
        .update(cx, |workspace, window, _| {
            window.activate_window();
            workspace.id()
        })
        .map_err(|_| Error::failed("the window has closed"))?;
    Ok(json!({"window": id.to_string()}))
}

/// Close the player windows but the one named, or the one in front.
fn close_others(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let keep = target::window(target, cx)?;
    for window in target::windows(cx) {
        if window != keep {
            // One closing as it's asked to closes anyway.
            window
                .update(cx, |_, window, _| window.remove_window())
                .ok();
        }
    }
    done()
}

/// Where the window named, or in front, is among the player windows, from
/// 0, and how many there are.
fn place(target: &Target, cx: &App) -> Result<(AnyWindowHandle, usize, usize), Error> {
    let window: AnyWindowHandle = target::window(target, cx)?.into();
    let windows = player_windows(cx);
    let at = windows
        .iter()
        .position(|&open| open == window)
        .ok_or_else(|| Error::failed("the window has closed"))?;
    Ok((window, at, windows.len()))
}

/// The number of the window named, or in front, from 1.
fn number(target: &Target, cx: &mut App) -> Outcome {
    let (_, at, _) = place(target, cx)?;
    Ok(json!(at + 1))
}

fn move_to_number(args: Args, target: &Target, cx: &mut App) -> Outcome {
    let Position { position } = args.read()?;
    let (window, _, count) = place(target, cx)?;
    if position > count {
        return Err(Error::bad_args(
            args.command,
            format!("position is at most {count}, as many as there are windows"),
        ));
    }
    move_window(window, position - 1, cx);
    done()
}

/// Move the window named, or in front, `step` places in the numbering, if
/// that's a place: the first stays first, the last last.
fn step_number(target: &Target, step: isize, cx: &mut App) -> Outcome {
    let (window, at, _) = place(target, cx)?;
    if let Some(to) = at.checked_add_signed(step) {
        move_window(window, to, cx);
    }
    done()
}

/// Close the player window named, else the window in front, whatever it
/// is, else the player window in front.
fn close(_: Args, target: &Target, cx: &mut App) -> Outcome {
    let window: AnyWindowHandle = match target.window {
        Some(_) => target::window(target, cx)?.into(),
        None => cx
            .active_window()
            .or_else(|| target::front_window(cx).map(Into::into))
            .ok_or_else(|| Error::not_found("window", "in front"))?,
    };
    window
        .update(cx, |_, window, _| window.remove_window())
        .map_err(|_| Error::failed("the window has closed"))?;
    done()
}

fn toggle_float_on_top(mode: FloatOnTop, cx: &mut App) {
    Settings::update(cx, true, |s| s.float_on_top = s.float_on_top.toggled(mode));
}

fn floats(mode: FloatOnTop, cx: &App) -> bool {
    cx.global::<Settings>().float_on_top == mode
}

/// Bring player window `number`, from 1 in the order they opened, to the
/// front: by its number alone (`#`), none if there's none, whatever is
/// labeled so or has an id ending so.
pub fn focus(number: usize) -> Run {
    Run(Invocation::new("window.focus")
        .in_window(format!("#{number}"))
        .from(Source::Action))
}

/// Give the window in front the number `number`, from 1.
pub fn move_to(number: usize) -> Run {
    Run::with("window.move", json!({"position": number}))
}

/// Show the window in front full screen on the display `id`.
pub fn show_on_display(id: u32) -> Run {
    Run::with("window.show_on_display", json!({"display": id}))
}
