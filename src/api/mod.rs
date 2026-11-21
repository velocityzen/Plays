//! The API: every command Plays has, by id, run one way. Keys, menus and
//! the controls that send actions run them through here, sending one
//! action, `Run`, which carries the command; the command palette and the
//! command line are to as well.
//!
//! A command (`registry`, with the commands themselves in `commands`) has
//! an id such as `playback.play_pause`, a title, a description, keywords,
//! its keys, the arguments it takes (`args`) and what it acts on: the app,
//! a window, every tile in a window, or a tile, by default the window in
//! front and the tile in it that keys go to, or as an invocation names
//! them (`target`). It hands back a JSON value, or an `Error`.

mod args;
pub mod commands;
mod error;
mod registry;
mod target;

use gpui::{App, Context, Global, Window};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use error::Error;
pub use registry::{Reach, Spec, all, find};
pub use target::{Id, Target, front_window};

use crate::ui::workspace::Workspace;

/// What keys, menu items and the controls that send actions send: run
/// this. One action for every command, so a command needs nothing more
/// to take keys or be in a menu.
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = plays, no_json)]
pub struct Run(pub Invocation);

impl Run {
    /// `command`, with no arguments, on what's in front.
    pub fn command(command: &str) -> Self {
        Self(Invocation::new(command).from(Source::Action))
    }

    /// `command`, given `args`, on what's in front.
    pub fn with(command: &str, args: Value) -> Self {
        Self(
            Invocation::new(command)
                .with_args(args)
                .from(Source::Action),
        )
    }
}

/// Run every `Run` sent, and enable a menu item only when its command can
/// run.
pub fn register(cx: &mut App) {
    cx.on_action(|run: &Run, cx| dispatch(run.0.clone(), cx));
    cx.validate_menu_actions_with(|action, cx| {
        let run = action.as_any().downcast_ref::<Run>()?;
        Some(can_run(&run.0.command, &run.0.target, cx))
    });
}

/// A command to run, with its arguments, what it's aimed at, and where it
/// came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Invocation {
    pub command: String,
    /// An object of them, or null for none.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub args: Value,
    #[serde(default, skip_serializing_if = "Target::is_empty")]
    pub target: Target,
    pub source: Source,
}

/// Where an invocation came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A key, a menu item, or a control that sends `Run`.
    Action,
    /// A control that runs it itself.
    Ui,
    Palette,
    Cli,
}

impl Invocation {
    /// `command`, with no arguments, aimed at the window in front, as if
    /// from a control.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Value::Null,
            target: Target::default(),
            source: Source::Ui,
        }
    }

    pub fn with_args(mut self, args: Value) -> Self {
        self.args = args;
        self
    }

    /// Aimed at the window `name` names.
    pub fn in_window(mut self, name: impl Into<String>) -> Self {
        self.target.window = Some(name.into());
        self
    }

    /// Aimed at the tile `name` names.
    pub fn on_tile(mut self, name: impl Into<String>) -> Self {
        self.target.tile = Some(name.into());
        self
    }

    pub fn from(mut self, source: Source) -> Self {
        self.source = source;
        self
    }
}

/// The command run last, and the one run before it.
#[derive(Default)]
struct Ran {
    last: Option<&'static str>,
    before: Option<&'static str>,
}
impl Global for Ran {}

/// The command run before the one running now, for one that goes on
/// from where it left off when run again with nothing run between, as Go
/// Back One Window walks back.
pub fn ran_before(cx: &App) -> Option<&'static str> {
    cx.try_global::<Ran>().and_then(|ran| ran.before)
}

/// Run `invocation` now, handing back what the command does.
///
/// Not while a window is being updated: not from an action's or a view's
/// listener, where gpui holds the window the event came to, which the
/// command may need. `dispatch` runs it once the event is handled.
pub fn run(invocation: Invocation, cx: &mut App) -> Result<Value, Error> {
    #[cfg(test)]
    if cx.has_global::<Recorder>() {
        let recorder = cx.global_mut::<Recorder>();
        recorder.seen.push(invocation.clone());
        if !recorder.runs {
            return Ok(Value::Null);
        }
    }
    let Invocation {
        command,
        args,
        target,
        ..
    } = invocation;
    let spec = find(&command).ok_or(Error::NoSuchCommand { command })?;
    args::check(spec.id, spec.args, &args)?;
    spec.check_target(&target)?;
    if !(spec.available)() {
        return Err(Error::failed(format!(
            "{} isn't on this system",
            spec.title
        )));
    }
    let ran = cx.default_global::<Ran>();
    ran.before = ran.last.replace(spec.id);
    let args = registry::Args {
        command: spec.id,
        value: &args,
    };
    match spec.run {
        registry::Run::App(run) => {
            run(cx);
            Ok(Value::Null)
        }
        registry::Run::AppWith(run) => run(args, cx),
        registry::Run::Window(run) => target::in_window(&target, cx, |workspace, window, cx| {
            run(workspace, window, cx);
            Ok(Value::Null)
        }),
        registry::Run::WindowWith(run) => {
            target::in_window(&target, cx, |workspace, window, cx| {
                run(args, workspace, window, cx)
            })
        }
        registry::Run::Tile(run) => target::on_tile(&target, cx, |player, window, cx| {
            run(player, window, cx);
            Ok(Value::Null)
        }),
        registry::Run::TileWith(run) => target::on_tile(&target, cx, |player, window, cx| {
            run(args, player, window, cx)
        }),
        registry::Run::Custom(run) => run(args, &target, cx),
    }
}

/// Every invocation run while it's set, as tests see what controls send.
#[cfg(test)]
struct Recorder {
    seen: Vec<Invocation>,
    /// Whether they run, or are only noted.
    runs: bool,
}

#[cfg(test)]
impl gpui::Global for Recorder {}

/// Note every invocation run from now on, for `recorded`, and run it.
#[cfg(test)]
pub fn record(cx: &mut App) {
    cx.set_global(Recorder {
        seen: Vec::new(),
        runs: true,
    });
}

/// Note every invocation from now on, for `recorded`, but run none: what
/// controls send, without what it does.
#[cfg(test)]
pub fn intercept(cx: &mut App) {
    cx.set_global(Recorder {
        seen: Vec::new(),
        runs: false,
    });
}

/// The invocations since `record` or `intercept`, or since last asked.
#[cfg(test)]
pub fn recorded(cx: &mut App) -> Vec<Invocation> {
    std::mem::take(&mut cx.global_mut::<Recorder>().seen)
}

/// Check `args` against what `spec` takes.
#[cfg(test)]
pub fn check_args(spec: &registry::Spec, args: &Value) -> Result<(), Error> {
    args::check(spec.id, spec.args, args)
}

/// Run `invocation` once the current event is handled, as keys, menus and
/// controls do, noting why if it doesn't run; then the menus check what
/// it changed.
pub fn dispatch(invocation: Invocation, cx: &mut App) {
    cx.defer(move |cx| {
        let command = invocation.command.clone();
        match run(invocation, cx) {
            Ok(_) => {}
            // A key pressed with no file open, say.
            Err(err) if err.is_not_found() => log::debug!("{command}: {err}"),
            Err(err) => log::warn!("{command}: {err}"),
        }
        crate::app::refresh_menus(cx);
    });
}

/// Run `f` on the window `target` names, or the one in front, once the
/// current event is handled, as `dispatch` runs a command: for what the app
/// does there that is no command, asking for an output's PIN, say.
pub fn in_window_later(
    target: Target,
    cx: &mut App,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) {
    cx.defer(move |cx| {
        let done = target::in_window(&target, cx, |workspace, window, cx| {
            f(workspace, window, cx);
            Ok(())
        });
        if let Err(err) = done {
            log::warn!("can't reach the window: {err}");
        }
    });
}

/// What `command` changes, aimed at `target`, as it is now: whether Loop
/// is on, say. Null for a command that changes nothing it can read.
pub fn state(command: &str, target: &Target, cx: &mut App) -> Result<Value, Error> {
    let spec = find(command).ok_or_else(|| Error::NoSuchCommand {
        command: command.to_owned(),
    })?;
    spec.check_target(target)?;
    match spec.state {
        Some(state) => state(target, cx),
        None => Ok(Value::Null),
    }
}

/// Whether `command` can run now, aimed at `target`: this system has what
/// it does, and there's what it acts on.
pub fn can_run(command: &str, target: &Target, cx: &mut App) -> bool {
    let Some(spec) = find(command) else {
        return false;
    };
    let there = match spec.scope {
        registry::Scope::App => true,
        registry::Scope::AnyWindow => {
            target.window.is_none() && cx.active_window().is_some()
                || target::window(target, cx).is_ok()
        }
        registry::Scope::Window | registry::Scope::Tiles => target::window(target, cx).is_ok(),
        registry::Scope::Tile => target::tile(target, cx).is_ok(),
    };
    there && spec.check_target(target).is_ok() && (spec.available)()
}

#[cfg(test)]
mod tests;
