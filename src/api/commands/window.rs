//! `window`: player windows: opening files into one, going from one to
//! another, closing one, full screen and the display it's on, and floating
//! them on top.

use gpui::{AnyWindowHandle, App};
use serde::Deserialize;
use serde_json::json;

use super::done;
use crate::api::args::{Arg, Kind};
use crate::api::registry::{Args, Outcome, Reach, Scope, Spec, on_mac};
use crate::api::target::{self, Target};
use crate::api::{Error, Invocation, Run, Source};
use crate::app::step_window;
use crate::settings::{FloatOnTop, Settings};

pub const COMMANDS: &[Spec] = &[
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
        "window.close",
        "Close Window",
        "Close the window and the files in it; with none named, the window in front, \
         Settings and About too.",
        Scope::AnyWindow,
        close,
    )
    // ⌘W closes the file in front, and this where there's none.
    .keys(&["secondary-alt-w", "secondary-w"]),
    Spec::window(
        "window.focus",
        "Go to Window",
        "Bring the window to the front. ⌘1 to ⌘9 go to the first nine, in the order \
         they opened.",
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

#[derive(Deserialize)]
struct OnDisplay {
    display: u32,
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
/// front.
pub fn focus(number: usize) -> Run {
    Run(Invocation::new("window.focus")
        .in_window(number.to_string())
        .from(Source::Action))
}

/// Show the window in front full screen on the display `id`.
pub fn show_on_display(id: u32) -> Run {
    Run::with("window.show_on_display", json!({"display": id}))
}
