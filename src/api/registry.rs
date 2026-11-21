//! What each command is: its id, title, description and keywords, what it
//! acts on, the arguments it takes, its keys, and what it does. The
//! commands themselves are in `commands`, a file for each namespace.

use gpui::{App, Context, Window};
use serde_json::{Value, json};

use super::Error;
use super::args::{self, Arg};
use super::commands;
use super::target::Target;
use crate::ui::player::PlayerView;
use crate::ui::workspace::Workspace;

/// What a command hands back, or why it didn't run.
pub type Outcome = Result<Value, Error>;

/// What a command acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The app: it takes no window or tile.
    App,
    /// A window, Settings and About too: the one in front, or the player
    /// window named.
    AnyWindow,
    /// A player window: the one in front, or the one named.
    Window,
    /// Every tile in a player window.
    Tiles,
    /// A tile: the one keys go to in the window in front, or the one named.
    Tile,
}

impl Scope {
    fn name(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::AnyWindow | Self::Window => "window",
            Self::Tiles => "tiles",
            Self::Tile => "tile",
        }
    }
}

/// Where a command's keys work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// In any window, Settings and About too.
    Anywhere,
    /// In a player window with files open, typing in a field there too.
    Files,
    /// In a player window with files open, but not while typing in a
    /// field there, where single keys such as space type.
    NotTyping,
}

/// A command's arguments, as given, and the command they were given to.
#[derive(Clone, Copy)]
pub struct Args<'a> {
    pub command: &'static str,
    pub value: &'a Value,
}

impl Args<'_> {
    /// Them as the command's own type; they've been checked already.
    pub fn read<T: serde::de::DeserializeOwned>(self) -> Result<T, Error> {
        args::read(self.command, self.value)
    }
}

type WindowFn = fn(&mut Workspace, &mut Window, &mut Context<Workspace>);
type WindowWithFn = fn(Args, &mut Workspace, &mut Window, &mut Context<Workspace>) -> Outcome;
type TileFn = fn(&mut PlayerView, &mut Window, &mut Context<PlayerView>);
type TileWithFn = fn(Args, &mut PlayerView, &mut Window, &mut Context<PlayerView>) -> Outcome;
type CustomFn = fn(Args, &Target, &mut App) -> Outcome;
/// Reads the state of what a command changes, for its target.
pub type StateFn = fn(&Target, &mut App) -> Outcome;

/// What a command does, given what it acts on. Those with arguments, or
/// something to hand back, are `…With`.
#[derive(Clone, Copy)]
pub enum Run {
    App(fn(&mut App)),
    AppWith(fn(Args, &mut App) -> Outcome),
    Window(WindowFn),
    WindowWith(WindowWithFn),
    Tile(TileFn),
    TileWith(TileWithFn),
    /// Finds what it acts on itself.
    Custom(CustomFn),
}

/// A command.
pub struct Spec {
    /// `namespace.name`, such as `playback.play_pause`.
    pub id: &'static str,
    pub title: &'static str,
    /// A sentence saying what it does.
    pub description: &'static str,
    /// Other words it may be looked for by.
    pub keywords: &'static [&'static str],
    pub scope: Scope,
    pub args: &'static [Arg],
    /// The keys it starts out with: `secondary` is ⌘ on macOS and Ctrl
    /// elsewhere. Settings can change them.
    pub keys: &'static [&'static str],
    /// The name the settings file keeps its keys under, if not its id:
    /// that of the shortcut it was before commands had ids.
    pub setting: Option<&'static str>,
    pub reach: Reach,
    /// Whether it must be told what to act on, as it does nothing to what's
    /// in front; such a command takes no keys of its own.
    pub needs_target: bool,
    pub run: Run,
    /// What it changes, as it is now: whether Loop is on, say.
    pub state: Option<StateFn>,
    /// What it hands back, as JSON, if anything: what the command reference
    /// says of it.
    pub returns: Option<&'static str>,
    /// Whether it only reads, as `window.ls` does: such a command takes no
    /// keys, and doesn't count as one run between two others, as Go Back
    /// One Window's walk sees them.
    pub query: bool,
    /// Whether this system has what it does.
    pub available: fn() -> bool,
}

impl Spec {
    const fn new(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        scope: Scope,
        run: Run,
    ) -> Self {
        Self {
            id,
            title,
            description,
            keywords: &[],
            scope,
            args: &[],
            keys: &[],
            setting: None,
            // The app's work anywhere; a window's or tile's where there are
            // files, but not while typing.
            reach: match scope {
                Scope::App | Scope::AnyWindow => Reach::Anywhere,
                Scope::Window | Scope::Tiles | Scope::Tile => Reach::NotTyping,
            },
            needs_target: false,
            run,
            state: None,
            returns: None,
            query: false,
            available: || true,
        }
    }

    pub const fn app(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        run: fn(&mut App),
    ) -> Self {
        Self::new(id, title, description, Scope::App, Run::App(run))
    }

    pub const fn app_with(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        args: &'static [Arg],
        run: fn(Args, &mut App) -> Outcome,
    ) -> Self {
        Self::new(id, title, description, Scope::App, Run::AppWith(run)).args(args)
    }

    pub const fn window(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        run: WindowFn,
    ) -> Self {
        Self::new(id, title, description, Scope::Window, Run::Window(run))
    }

    pub const fn window_with(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        args: &'static [Arg],
        run: WindowWithFn,
    ) -> Self {
        Self::new(id, title, description, Scope::Window, Run::WindowWith(run)).args(args)
    }

    /// One for every tile in a window.
    pub const fn tiles(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        run: WindowFn,
    ) -> Self {
        Self::new(id, title, description, Scope::Tiles, Run::Window(run))
    }

    pub const fn tile(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        run: TileFn,
    ) -> Self {
        Self::new(id, title, description, Scope::Tile, Run::Tile(run))
    }

    pub const fn tile_with(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        args: &'static [Arg],
        run: TileWithFn,
    ) -> Self {
        Self::new(id, title, description, Scope::Tile, Run::TileWith(run)).args(args)
    }

    /// One that finds what it acts on, a `scope`, itself.
    pub const fn custom(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        scope: Scope,
        run: CustomFn,
    ) -> Self {
        Self::new(id, title, description, scope, Run::Custom(run))
    }

    /// One that finds what it acts on, a `scope`, itself, given `args`.
    pub const fn custom_with(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        scope: Scope,
        args: &'static [Arg],
        run: CustomFn,
    ) -> Self {
        Self::new(id, title, description, scope, Run::Custom(run)).args(args)
    }

    /// One that only reads what `scope` is, and hands back what `returns`
    /// says: listing the windows, say.
    pub const fn query(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        scope: Scope,
        returns: &'static str,
        run: CustomFn,
    ) -> Self {
        let mut spec = Self::custom(id, title, description, scope, run);
        spec.returns = Some(returns);
        spec.query = true;
        spec
    }

    pub const fn returns(mut self, returns: &'static str) -> Self {
        self.returns = Some(returns);
        self
    }

    pub const fn keywords(mut self, keywords: &'static [&'static str]) -> Self {
        self.keywords = keywords;
        self
    }

    const fn args(mut self, args: &'static [Arg]) -> Self {
        self.args = args;
        self
    }

    pub const fn keys(mut self, keys: &'static [&'static str]) -> Self {
        self.keys = keys;
        self
    }

    pub const fn setting(mut self, setting: &'static str) -> Self {
        self.setting = Some(setting);
        self
    }

    pub const fn reach(mut self, reach: Reach) -> Self {
        self.reach = reach;
        self
    }

    pub const fn needs_target(mut self) -> Self {
        self.needs_target = true;
        self
    }

    pub const fn state(mut self, state: StateFn) -> Self {
        self.state = Some(state);
        self
    }

    pub const fn only_if(mut self, available: fn() -> bool) -> Self {
        self.available = available;
        self
    }

    /// The first part of its id.
    pub fn namespace(&self) -> &'static str {
        self.id
            .split_once('.')
            .map_or(self.id, |(namespace, _)| namespace)
    }

    /// Whether it can be given keys: it changes something, needs no
    /// arguments or target, and this system has what it does.
    pub fn takes_keys(&self) -> bool {
        !self.query && self.args.is_empty() && !self.needs_target && (self.available)()
    }

    /// The name the settings file keeps its keys under.
    pub fn setting_name(&self) -> &'static str {
        self.setting.unwrap_or(self.id)
    }

    /// Whether `target` names what it can act on: nothing, for the app's;
    /// no tile, for a window's.
    pub fn check_target(&self, target: &Target) -> Result<(), Error> {
        let message = match self.scope {
            Scope::App if !target.is_empty() => "acts on the app, not on a window or tile",
            Scope::AnyWindow | Scope::Window | Scope::Tiles if target.tile.is_some() => {
                "acts on a window: name it with window, not tile"
            }
            _ => return Ok(()),
        };
        Err(Error::bad_args(self.id, message))
    }

    /// All it is, as `plays api describe` says.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "for the command line and palette to come")
    )]
    pub fn describe(&self) -> Value {
        json!({
            "id": self.id,
            "title": self.title,
            "description": self.description,
            "keywords": self.keywords,
            "namespace": self.namespace(),
            "scope": self.scope.name(),
            "keys": self.keys,
            "setting": self.takes_keys().then(|| self.setting_name()),
            "args": self.args.iter().map(Arg::describe).collect::<Vec<_>>(),
            "state": self.state.is_some(),
            "returns": self.returns,
            "query": self.query,
            "available": (self.available)(),
        })
    }
}

/// Every command, by namespace.
pub fn all() -> impl Iterator<Item = &'static Spec> {
    commands::ALL.iter().flat_map(|commands| commands.iter())
}

/// The command `id`.
pub fn find(id: &str) -> Option<&'static Spec> {
    all().find(|spec| spec.id == id)
}

/// `mac` on macOS, else `other`: keys that differ.
pub const fn on_mac(
    mac: &'static [&'static str],
    other: &'static [&'static str],
) -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}
