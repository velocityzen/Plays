//! `window`: player windows: opening one, or files into one, naming one,
//! going from one to another and back, numbering them, closing one or the
//! others, full screen and the display it's on, and floating them on top.

use gpui::{AnyWindowHandle, App, Context, Window};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Switch, done, panels};
use crate::api::args::{Arg, Kind};
use crate::api::events::{EventField, EventKind};
use crate::api::registry::{Args, Outcome, Reach, Scope, Spec, on_mac};
use crate::api::target::{self, Target};
use crate::api::{Error, Invocation, Run, Source};
use crate::app::{back_window, move_window, player_windows, recent_window, step_window};
use crate::recents;
use crate::settings::{FloatOnTop, OverVideo, Panel, Settings};
use crate::ui::palette::{self, Listing};
use crate::ui::theme::layout::WINDOW_SIZE;
use crate::ui::workspace::Workspace;

pub const COMMANDS: &[Spec] = &[
    Spec::query(
        "window.ls",
        "List Windows",
        "List the player windows, in the order ⌘1 to ⌘9 number them.",
        Scope::App,
        "[{id, label, title, number, front, blocks}]: each window's id, label (or null) \
         and title, as its title bar shows it, its number from 1, whether it's in front, \
         and how many blocks it has.",
        |_, _, cx| Ok(Value::from(summaries(cx)?)),
    )
    .keywords(&["list", "ls", "windows"]),
    Spec::query(
        "window.inspect",
        "Inspect Window",
        "Say all about the window: where it is, how it shows and its blocks.",
        Scope::Window,
        "{id, label, title, number, front, blocks: [as block.ls], bounds: {x, y, width, height}, \
         display, full_screen, on_top, visible}: as window.ls says, its blocks as block.ls \
         lists them, where it is in points, the id of the display it's on, whether it's \
         full screen and floats on top, and the panels over the video it shows in blocks \
         opened in it, of controls, top_bar, track_switcher and notes.",
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
    .tells(&["window.created", "window.active_changed"])
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
        "List the windows, their blocks, the files in the front block's folder and those \
         played lately, to find one by name and go there: bring it to the front, play \
         the file in the block, or open it.",
        Scope::Window,
        show_go_to,
    )
    .keywords(&[
        "switch",
        "find",
        "files",
        "recent",
        "windows",
        "blocks",
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
    .tells(&["block.closed", "window.closed", "window.active_changed"])
    // ⌘W closes the file in front, and this where there's none.
    .keys(&["secondary-alt-w", "secondary-w"]),
    Spec::custom(
        "window.close_others",
        "Close Other Windows",
        "Close every player window but this one, and the files in them.",
        Scope::Window,
        close_others,
    )
    .tells(&["block.closed", "window.closed"])
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
    .tells(&["window.label_changed"])
    .keywords(&["name", "label", "title"]),
    Spec::window(
        "window.focus",
        "Go to Window",
        "Bring the window to the front. ⌘1 to ⌘9 go to the first nine, by their \
         numbers.",
        |_, window, _| window.activate_window(),
    )
    .tells(&["window.active_changed"])
    .keywords(&["switch", "show", "raise"])
    // The window in front is in front already; ⌘1 to ⌘9 name theirs.
    .needs_target(),
    Spec::app(
        "window.next",
        "Next Window",
        "Bring the window after the one in front to the front: the first after the last.",
        |cx| step_window(1, cx),
    )
    .tells(&["window.active_changed"])
    .keywords(&["switch", "cycle"])
    .keys(&["ctrl-tab"]),
    Spec::app(
        "window.previous",
        "Previous Window",
        "Bring the window before the one in front to the front: the last before the first.",
        |cx| step_window(-1, cx),
    )
    .tells(&["window.active_changed"])
    .keywords(&["switch", "cycle"])
    .keys(&["ctrl-shift-tab"]),
    Spec::app(
        "window.back",
        "Go Back One Window",
        "Bring back the player window in front before this one; again, the one before \
         that, and so on, until something else is done.",
        back_window,
    )
    .tells(&["window.active_changed"])
    .keywords(&["history", "previous", "last", "return"]),
    Spec::app(
        "window.recent",
        "Go to Recent Window",
        "Bring back the player window in front last; again, the one in front first.",
        recent_window,
    )
    .tells(&["window.active_changed"])
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
        "window.full_screen.toggle",
        "Full Screen",
        "Show the window full screen, or as it was again.",
        |_, window, _| switch_full_screen(Switch::Toggle, window),
    )
    .tells(&["window.full_screen.on", "window.full_screen.off"])
    .keywords(&["enter", "exit", "maximize"])
    .setting("full_screen")
    .keys(on_mac(&["ctrl-cmd-f", "f"], &["f11", "f"]))
    .state(|target, cx| {
        target::in_window(target, cx, |_, window, _| Ok(json!(window.is_fullscreen())))
    }),
    Spec::window(
        "window.full_screen.on",
        "Enter Full Screen",
        "Show the window full screen, unless it is already.",
        |_, window, _| switch_full_screen(Switch::On, window),
    )
    .tells(&["window.full_screen.on"])
    .keywords(&["maximize"]),
    Spec::window(
        "window.full_screen.off",
        "Exit Full Screen",
        "Show the window as it was before it was full screen.",
        |_, window, _| switch_full_screen(Switch::Off, window),
    )
    .tells(&["window.full_screen.off"])
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
        "window.float_on_top.toggle",
        "Float on Top",
        "Keep player windows above other apps' windows, or stop.",
        |cx| switch_float_on_top(FloatOnTop::Always, Switch::Toggle, cx),
    )
    .tells(&[
        "app.settings.changed",
        "window.float_on_top.on",
        "window.float_on_top.off",
    ])
    .keywords(&["pin", "always on top", "above"])
    .setting("float_on_top")
    .state(|_, cx| Ok(json!(floats(FloatOnTop::Always, cx)))),
    Spec::app(
        "window.float_on_top.on",
        "Start Floating on Top",
        "Keep player windows above other apps' windows, unless they are already.",
        |cx| switch_float_on_top(FloatOnTop::Always, Switch::On, cx),
    )
    .tells(&["app.settings.changed", "window.float_on_top.on"])
    .keywords(&["pin", "always on top", "above"]),
    Spec::app(
        "window.float_on_top.off",
        "Stop Floating on Top",
        "Stop keeping player windows above other apps' windows, unless they float on top \
         while playing.",
        |cx| switch_float_on_top(FloatOnTop::Always, Switch::Off, cx),
    )
    .tells(&["app.settings.changed", "window.float_on_top.off"])
    .keywords(&["pin", "always on top", "above"]),
    Spec::app(
        "window.float_on_top_while_playing.toggle",
        "Float on Top While Playing",
        "Keep player windows above other apps' windows while a file in them plays, or stop.",
        |cx| switch_float_on_top(FloatOnTop::WhilePlaying, Switch::Toggle, cx),
    )
    .tells(&[
        "app.settings.changed",
        "window.float_on_top_while_playing.on",
        "window.float_on_top_while_playing.off",
    ])
    .keywords(&["pin", "always on top", "above"])
    .setting("float_on_top_while_playing")
    .state(|_, cx| Ok(json!(floats(FloatOnTop::WhilePlaying, cx)))),
    Spec::app(
        "window.float_on_top_while_playing.on",
        "Start Floating on Top While Playing",
        "Keep player windows above other apps' windows while a file in them plays, unless \
         they are already.",
        |cx| switch_float_on_top(FloatOnTop::WhilePlaying, Switch::On, cx),
    )
    .tells(&[
        "app.settings.changed",
        "window.float_on_top_while_playing.on",
    ])
    .keywords(&["pin", "always on top", "above"]),
    Spec::app(
        "window.float_on_top_while_playing.off",
        "Stop Floating on Top While Playing",
        "Stop keeping player windows above other apps' windows while a file in them \
         plays, unless they float on top always.",
        |cx| switch_float_on_top(FloatOnTop::WhilePlaying, Switch::Off, cx),
    )
    .tells(&[
        "app.settings.changed",
        "window.float_on_top_while_playing.off",
    ])
    .keywords(&["pin", "always on top", "above"]),
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
    .tells(&["window.full_screen.on"])
    .keywords(&["screen", "monitor", "projector", "move"]),
    Spec::window(
        "window.ui.toggle",
        "Show/Hide All in Window",
        "Hide every panel over the video in each block in the window while the window \
         shows any, else show them all.",
        |workspace, _, cx| switch_panel(Switch::Toggle, None, workspace, cx),
    )
    .tells(TELLS_ALL)
    .keywords(&["hide", "interface", "ui", "panels", "clean"])
    .setting("window.toggle_ui"),
    Spec::window(
        "window.ui.on",
        "Show All in Window",
        "Show every panel over the video in each block in the window, and those opened \
         in it.",
        |workspace, _, cx| switch_panel(Switch::On, None, workspace, cx),
    )
    .tells(TELLS_ALL_ON)
    .keywords(&["interface", "ui", "panels"]),
    Spec::window(
        "window.ui.off",
        "Hide All in Window",
        "Hide every panel over the video in each block in the window, and those opened \
         in it.",
        |workspace, _, cx| switch_panel(Switch::Off, None, workspace, cx),
    )
    .tells(TELLS_ALL_OFF)
    .keywords(&["interface", "ui", "panels", "clean"]),
    Spec::window_with(
        "window.set_ui",
        "Show Panels in Window",
        "Show the panels named over the video in each block in the window, and those \
         opened in it, and hide the rest.",
        &[PANELS_SHOWN],
        |args, workspace, _, cx| {
            let ShowPanels { visible } = args.read()?;
            workspace.show_over_video(OverVideo::showing(&visible), cx);
            done()
        },
    )
    .tells(TELLS_ALL_OFF)
    .keywords(&["hide", "interface", "ui", "panels", "clean", "show"])
    .state(|target, cx| {
        target::in_window(target, cx, |workspace, _, _| {
            Ok(json!(workspace.over_video().visible()))
        })
    }),
    Spec::window(
        "window.controls.toggle",
        "Show/Hide Controls in Window",
        "Show the seek bar and buttons at the bottom of each block in the window, and \
         those opened in it, or hide them.",
        |workspace, _, cx| switch_panel(Switch::Toggle, Some(panels::controls), workspace, cx),
    )
    .tells(&[
        "window.controls.on",
        "window.controls.off",
        "block.controls.on",
        "block.controls.off",
    ])
    .keywords(&["hide", "player controls", "seek bar", "bottom bar"])
    .setting("window.toggle_controls")
    .state(|target, cx| shown_in_window(target, cx, |shown| shown.controls)),
    Spec::window(
        "window.controls.on",
        "Show Controls in Window",
        "Show the seek bar and buttons at the bottom of each block in the window, and \
         those opened in it.",
        |workspace, _, cx| switch_panel(Switch::On, Some(panels::controls), workspace, cx),
    )
    .tells(&["window.controls.on", "block.controls.on"])
    .keywords(&["player controls", "seek bar", "bottom bar"]),
    Spec::window(
        "window.controls.off",
        "Hide Controls in Window",
        "Hide the seek bar and buttons at the bottom of each block in the window, and \
         those opened in it.",
        |workspace, _, cx| switch_panel(Switch::Off, Some(panels::controls), workspace, cx),
    )
    .tells(&["window.controls.off", "block.controls.off"])
    .keywords(&["player controls", "seek bar", "bottom bar"]),
    Spec::window(
        "window.top_bar.toggle",
        "Show/Hide Top Bar in Window",
        "Show the file's name and buttons at the top of each block in the window, and \
         those opened in it, or hide them.",
        |workspace, _, cx| switch_panel(Switch::Toggle, Some(panels::top_bar), workspace, cx),
    )
    .tells(&[
        "window.top_bar.on",
        "window.top_bar.off",
        "block.top_bar.on",
        "block.top_bar.off",
    ])
    .keywords(&["hide", "header", "title", "buttons"])
    .setting("window.toggle_top_bar")
    .state(|target, cx| shown_in_window(target, cx, |shown| shown.top_bar)),
    Spec::window(
        "window.top_bar.on",
        "Show Top Bar in Window",
        "Show the file's name and buttons at the top of each block in the window, and \
         those opened in it.",
        |workspace, _, cx| switch_panel(Switch::On, Some(panels::top_bar), workspace, cx),
    )
    .tells(&["window.top_bar.on", "block.top_bar.on"])
    .keywords(&["header", "title", "buttons"]),
    Spec::window(
        "window.top_bar.off",
        "Hide Top Bar in Window",
        "Hide the file's name and buttons at the top of each block in the window, and \
         those opened in it.",
        |workspace, _, cx| switch_panel(Switch::Off, Some(panels::top_bar), workspace, cx),
    )
    .tells(&["window.top_bar.off", "block.top_bar.off"])
    .keywords(&["header", "title", "buttons"]),
    Spec::window(
        "window.track_switcher.toggle",
        "Show/Hide Track Switcher in Window",
        "Show the reel of tracks a shortcut rolls to the audio, video or subtitles \
         picked, over each block in the window, and those opened in it, or switch \
         without it.",
        |workspace, _, cx| {
            switch_panel(Switch::Toggle, Some(panels::track_switcher), workspace, cx)
        },
    )
    .tells(&[
        "window.track_switcher.on",
        "window.track_switcher.off",
        "block.track_switcher.on",
        "block.track_switcher.off",
    ])
    .keywords(&["hide", "reel", "streams", "audio track", "subtitle track"])
    .setting("window.toggle_track_switcher")
    .state(|target, cx| shown_in_window(target, cx, |shown| shown.track_switcher)),
    Spec::window(
        "window.track_switcher.on",
        "Show Track Switcher in Window",
        "Show the reel of tracks a shortcut rolls to the audio, video or subtitles \
         picked, over each block in the window, and those opened in it.",
        |workspace, _, cx| switch_panel(Switch::On, Some(panels::track_switcher), workspace, cx),
    )
    .tells(&["window.track_switcher.on", "block.track_switcher.on"])
    .keywords(&["reel", "streams", "audio track", "subtitle track"]),
    Spec::window(
        "window.track_switcher.off",
        "Hide Track Switcher in Window",
        "Switch tracks without the reel in each block in the window, and those opened \
         in it.",
        |workspace, _, cx| switch_panel(Switch::Off, Some(panels::track_switcher), workspace, cx),
    )
    .tells(&["window.track_switcher.off", "block.track_switcher.off"])
    .keywords(&["reel", "streams", "audio track", "subtitle track"]),
    Spec::window(
        "window.notes.toggle",
        "Show/Hide Notes in Window",
        "Show notes on what a shortcut changed, like the volume, over each block in the \
         window, and those opened in it, or not. Notes on what's wrong still show.",
        |workspace, _, cx| switch_panel(Switch::Toggle, Some(panels::notes), workspace, cx),
    )
    .tells(&[
        "window.notes.on",
        "window.notes.off",
        "block.notes.on",
        "block.notes.off",
    ])
    .keywords(&["hide", "osd", "on-screen display", "volume", "messages"])
    .setting("window.toggle_notes")
    .state(|target, cx| shown_in_window(target, cx, |shown| shown.notes)),
    Spec::window(
        "window.notes.on",
        "Show Notes in Window",
        "Show notes on what a shortcut changed, like the volume, over each block in the \
         window, and those opened in it.",
        |workspace, _, cx| switch_panel(Switch::On, Some(panels::notes), workspace, cx),
    )
    .tells(&["window.notes.on", "block.notes.on"])
    .keywords(&["osd", "on-screen display", "volume", "messages"]),
    Spec::window(
        "window.notes.off",
        "Hide Notes in Window",
        "Hide notes on what a shortcut changed over each block in the window, and those \
         opened in it. Notes on what's wrong still show.",
        |workspace, _, cx| switch_panel(Switch::Off, Some(panels::notes), workspace, cx),
    )
    .tells(&["window.notes.off", "block.notes.off"])
    .keywords(&["osd", "on-screen display", "volume", "messages"]),
];

/// What every panel over the video in a window showing or hiding tells.
const TELLS_ALL: &[&str] = &[
    "window.ui.on",
    "window.ui.off",
    "window.controls.on",
    "window.controls.off",
    "window.top_bar.on",
    "window.top_bar.off",
    "window.track_switcher.on",
    "window.track_switcher.off",
    "window.notes.on",
    "window.notes.off",
    "block.ui.on",
    "block.ui.off",
    "block.controls.on",
    "block.controls.off",
    "block.top_bar.on",
    "block.top_bar.off",
    "block.track_switcher.on",
    "block.track_switcher.off",
    "block.notes.on",
    "block.notes.off",
];

/// What every panel over the video in a window showing tells.
const TELLS_ALL_ON: &[&str] = &[
    "window.ui.on",
    "window.controls.on",
    "window.top_bar.on",
    "window.track_switcher.on",
    "window.notes.on",
    "block.ui.on",
    "block.controls.on",
    "block.top_bar.on",
    "block.track_switcher.on",
    "block.notes.on",
];

/// What every panel over the video in a window hiding tells.
const TELLS_ALL_OFF: &[&str] = &[
    "window.ui.off",
    "window.controls.off",
    "window.top_bar.off",
    "window.track_switcher.off",
    "window.notes.off",
    "block.ui.off",
    "block.controls.off",
    "block.top_bar.off",
    "block.track_switcher.off",
    "block.notes.off",
];

/// The panels over the video `window.set_ui` and `block.set_ui` show.
pub const PANELS_SHOWN: Arg = Arg::required(
    "visible",
    Kind::Choices(|| OverVideo::PANELS.to_vec()),
    "The panels to show; those not named hide.",
);

#[derive(Deserialize)]
pub struct ShowPanels {
    #[serde(deserialize_with = "crate::api::args::words")]
    pub visible: Vec<String>,
}

/// The windows' events.
pub const EVENTS: &[EventKind] = &[
    EventKind::of_window(
        "window.created",
        "A player window opened.",
        &[EventField::new("label", "Its label; none without one.")],
    ),
    EventKind::of_window(
        "window.closed",
        "A player window closed: told after each of its blocks is told closed.",
        &[],
    )
    .causes(&["Plays quitting"]),
    EventKind::of_window(
        "window.label_changed",
        "A window was labeled, or its label taken away.",
        &[EventField::new(
            "label",
            "Its label; none once it's taken away.",
        )],
    ),
    EventKind::of_window(
        "window.active_changed",
        "Another player window came to the front, or none is in front: its `window_id` and \
         `window_title` are none then.",
        &[],
    )
    .causes(&["a click on another window"]),
    EventKind::window("window.full_screen.on", "A window went full screen.", &[])
        .causes(&["the window's own full screen button"]),
    EventKind::window(
        "window.full_screen.off",
        "A window came back from full screen.",
        &[],
    )
    .causes(&["the window's own full screen button"]),
    EventKind::app(
        "window.float_on_top.on",
        "Player windows began to float above other apps' windows.",
        &[],
    )
    .causes(&[FLOAT_SET]),
    EventKind::app(
        "window.float_on_top.off",
        "Player windows stopped floating above other apps' windows.",
        &[],
    )
    .causes(&[FLOAT_SET]),
    EventKind::app(
        "window.float_on_top_while_playing.on",
        "Player windows began to float above other apps' windows while a file in them plays.",
        &[],
    )
    .causes(&[FLOAT_SET]),
    EventKind::app(
        "window.float_on_top_while_playing.off",
        "Player windows stopped floating above other apps' windows while a file in them plays.",
        &[],
    )
    .causes(&[FLOAT_SET]),
    EventKind::window(
        "window.ui.on",
        "A panel showed over the video in a window's blocks, and those opened in it, where none did.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.ui.off",
        "Every panel over the video in a window's blocks, and those opened in it, hid.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.controls.on",
        "The seek bar and buttons at the bottom of a window's blocks, and those opened in it, showed.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.controls.off",
        "The seek bar and buttons at the bottom of a window's blocks, and those opened in it, hid.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.top_bar.on",
        "The file's name and buttons at the top of a window's blocks, and those opened in it, showed.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.top_bar.off",
        "The file's name and buttons at the top of a window's blocks, and those opened in it, hid.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.track_switcher.on",
        "A window's blocks, and those opened in it, show the reel of tracks as a shortcut rolls to one.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.track_switcher.off",
        "A window's blocks, and those opened in it, switch tracks without the reel.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.notes.on",
        "A window's blocks, and those opened in it, show notes on what a shortcut changed.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.notes.off",
        "A window's blocks, and those opened in it, show no notes on what a shortcut changed.",
        &[],
    )
    .causes(&[super::PANELS_SET]),
    EventKind::window(
        "window.layout_changed",
        "Blocks came into a window, went, or changed places, or one filled it or stopped \
         filling it.",
        &[
            EventField::new(
                "blocks",
                "The ids of its blocks, in the order its grid shows them.",
            ),
            EventField::new(
                "maximized",
                "The id of the block filling it; none if none does.",
            ),
        ],
    )
    .causes(&[super::DROPPED]),
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

/// Show Go to… over the window, listing the windows, the blocks, the files
/// in the front block's folder and those played lately.
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
        block: None,
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

/// What floats player windows on top, or stops them, other than a
/// command: events' cause.
const FLOAT_SET: &str = "Settings → Appearance floating player windows on top, importing a \
     settings file or resetting every setting";

/// Show `window` full screen, or as it was, as `switch` says.
fn switch_full_screen(switch: Switch, window: &mut Window) {
    let full = window.is_fullscreen();
    if switch.turn(full) != full {
        window.toggle_fullscreen();
    }
}

/// Float player windows on top as `mode` does, or stop, as `switch` says,
/// and keep it so.
fn switch_float_on_top(mode: FloatOnTop, switch: Switch, cx: &mut App) {
    Settings::update(cx, true, |s| {
        s.float_on_top = s.float_on_top.switched(mode, |on| switch.turn(on));
    });
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

/// Show the panel `panel` picks over the video in `workspace`, and so in
/// its blocks, or hide it, as `switch` says; with none, every panel.
fn switch_panel(
    switch: Switch,
    panel: Option<Panel>,
    workspace: &mut Workspace,
    cx: &mut Context<Workspace>,
) {
    let shown = workspace.over_video().switched(panel, |on| switch.turn(on));
    workspace.set_over_video(shown, cx);
}

/// Whether the window `target` names, or the one in front, shows the panel
/// `panel` reads.
fn shown_in_window(target: &Target, cx: &mut App, panel: fn(OverVideo) -> bool) -> Outcome {
    target::in_window(target, cx, |workspace, _, _| {
        Ok(json!(panel(workspace.over_video())))
    })
}
