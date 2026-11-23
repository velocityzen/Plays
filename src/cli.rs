//! The command line: `plays ls`, `plays do playback.play_pause`, `plays
//! events`. It runs apart from the app, without its UI, so it answers at
//! once: it asks the Plays running over the socket (`api::socket`), and
//! starts one if none is, unless told not to (`--no-autostart`). What
//! comes back goes to stdout, as text or, with `--json`, as JSON, and why
//! not to stderr. The exit status says which, as Rex's do: 0 done, 1
//! failed, 2 a bad command line, 3 nothing by the name given, 4 no Plays
//! to ask, and 130 stopped with Ctrl-C, which nothing here catches.
//!
//! `plays FILE…`, with no subcommand, isn't the command line's: it's
//! Plays started with files, which hands them to the one running.

mod args;
mod completions;
mod events;
mod reference;
mod show;
mod windows;

#[cfg(test)]
#[path = "tests/cli.rs"]
mod tests;

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{CommandFactory, Parser};
use serde_json::{Map, Value, json};

use self::args::{Api, Cli, Command, Options, Settings};
use crate::api::args::Kind;
use crate::api::socket::client::{self, Client, Failure};
use crate::api::{self, Error, Target};

/// Whether `args` are for the command line: a subcommand or one of its
/// options comes first, or a word that names no file, a subcommand
/// mistyped. Files, folders and URLs are Plays', as is nothing.
pub fn wanted(args: &[OsString]) -> bool {
    let Some(first) = args.first().and_then(|arg| arg.to_str()) else {
        return false;
    };
    let cli = Cli::command();
    if let Some(long) = first.strip_prefix("--") {
        let long = long.split_once('=').map_or(long, |(long, _)| long);
        return ["help", "version"].contains(&long)
            || cli.get_arguments().any(|arg| arg.get_long() == Some(long));
    }
    if let Some(short) = first.strip_prefix('-') {
        let mut letters = short.chars();
        return letters.next().is_some_and(|letter| {
            ['h', 'V'].contains(&letter)
                || cli
                    .get_arguments()
                    .any(|arg| arg.get_short() == Some(letter))
        });
    }
    let word = !first.is_empty() && !first.contains(['/', '.', ':', '\\']);
    first == "help"
        || cli
            .get_subcommands()
            .any(|command| command.get_name() == first)
        || (word && !Path::new(first).exists())
}

/// Run the command line `args` asks for, as this process: on the socket
/// Plays answers on, starting it if need be, printing to stdout and
/// stderr. Its exit status.
pub fn main(args: Vec<OsString>) -> u8 {
    let start = client::start_plays;
    run(
        args,
        api::socket::path(),
        Some(&start),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}

/// Run the command line `args` asks for, asking the Plays answering at
/// `socket`, and starting one with `start`, if given, if none does. Its
/// exit status.
pub fn run(
    args: Vec<OsString>,
    socket: Option<PathBuf>,
    start: Option<&dyn Fn() -> io::Result<()>>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let cli = match Cli::try_parse_from(std::iter::once(OsString::from("plays")).chain(args)) {
        Ok(cli) => cli,
        // Help and the version too, which aren't errors.
        Err(parse) if parse.use_stderr() => {
            write!(err, "{}", parse.render()).ok();
            return 2;
        }
        Err(parse) => {
            write!(out, "{}", parse.render()).ok();
            return 0;
        }
    };
    let mut session = Session {
        options: cli.options,
        socket,
        start,
        out,
        client: None,
    };
    match session.run(cli.command) {
        Ok(()) => 0,
        Err(Stop::Output(broken)) if broken.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(stop) => {
            writeln!(err, "plays: {stop}").ok();
            stop.exit_status()
        }
    }
}

/// Why a command line stopped short.
#[derive(Debug)]
enum Stop {
    /// Plays said no, or couldn't be reached.
    Failed(Failure),
    /// The command line asks for what can't be.
    Usage(String),
    /// What came back couldn't be printed: stdout closed, as `| head`
    /// closes it, say.
    Output(io::Error),
}

impl Stop {
    fn exit_status(&self) -> u8 {
        match self {
            Self::Failed(failure) => failure.exit_status(),
            Self::Usage(_) => 2,
            Self::Output(_) => 1,
        }
    }
}

impl std::fmt::Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed(failure) => failure.fmt(f),
            Self::Usage(why) => f.write_str(why),
            Self::Output(err) => write!(f, "can't print: {err}"),
        }
    }
}

impl From<Failure> for Stop {
    fn from(failure: Failure) -> Self {
        Self::Failed(failure)
    }
}

impl From<io::Error> for Stop {
    fn from(err: io::Error) -> Self {
        Self::Output(err)
    }
}

impl From<Error> for Stop {
    fn from(err: Error) -> Self {
        Self::Failed(Failure::Command(err))
    }
}

type Done = Result<(), Stop>;

/// A command line running: what it was given, where it prints, and
/// Plays, once asked.
struct Session<'a> {
    options: Options,
    socket: Option<PathBuf>,
    start: Option<&'a dyn Fn() -> io::Result<()>>,
    out: &'a mut dyn Write,
    client: Option<Client>,
}

impl Session<'_> {
    fn run(&mut self, command: Command) -> Done {
        match command {
            Command::Do { command, args } => {
                let args = arguments(&command, &args)?;
                let result = self.call(&command, args)?;
                self.result(&result)
            }
            Command::Actions { search, markdown } => {
                reference::actions(self.out, self.options.json, markdown, search.as_deref())
            }
            Command::Api(api) => self.api(api),
            Command::Ls => windows::ls(self),
            Command::Window(window) => windows::window(self, window),
            Command::Block(block) => windows::block(self, block),
            Command::Open { paths, new_window } => self.open(&paths, new_window),
            Command::Events { positions, count } => events::events(self, positions, count),
            Command::Wait { event, timeout } => events::wait(self, &event, timeout),
            Command::Settings(settings) => self.settings(settings),
            Command::Keymap => self.keymap(),
            Command::Version => self.version(),
            Command::Completions { shell } => {
                completions::completions(self.out, shell).map_err(Stop::Output)
            }
        }
    }

    /// Plays, connected to, or started first.
    fn client(&mut self) -> Result<&mut Client, Failure> {
        if self.client.is_none() {
            let Some(path) = self.socket.as_deref() else {
                let none = "this build has no socket of its own: name one with PLAYS_SOCKET";
                return Err(io::Error::new(io::ErrorKind::NotFound, none).into());
            };
            let start = self.start.filter(|_| !self.options.no_autostart);
            self.client = Some(client::connect_or_start(path, start)?);
        }
        Ok(self.client.as_mut().expect("connected"))
    }

    /// What `-w` and `-b` name.
    fn target(&self) -> Target {
        Target {
            window: self.options.window.clone(),
            block: self.options.block.clone(),
        }
    }

    /// Run `command` with `args` on what `-w` and `-b` name.
    fn call(&mut self, command: &str, args: Value) -> Result<Value, Failure> {
        let target = self.target();
        self.call_on(command, args, target)
    }

    /// Run `command` with `args` on what `target` names.
    fn call_on(&mut self, command: &str, args: Value, target: Target) -> Result<Value, Failure> {
        self.client()?.call(command, args, target)
    }

    /// Print what a command handed back.
    fn result(&mut self, value: &Value) -> Done {
        Ok(show::result(self.out, value, self.options.json)?)
    }

    fn api(&mut self, api: Api) -> Done {
        match api {
            Api::List => {
                let commands = self.client()?.commands()?;
                if self.options.json {
                    return self.result(&commands);
                }
                let rows: Vec<Vec<String>> = listed(&commands)
                    .map(|spec| vec![show::text(&spec["id"]), show::text(&spec["title"])])
                    .collect();
                Ok(show::table(self.out, &["ID", "TITLE"], &rows)?)
            }
            Api::Describe { command } => {
                let described = self.client()?.describe(&command)?;
                Ok(show::pretty(self.out, &described)?)
            }
            Api::Call { command, args } => {
                let args = match args {
                    Some(args) => serde_json::from_str(&args)
                        .map_err(|err| Stop::Usage(format!("the arguments aren't JSON: {err}")))?,
                    None => Value::Null,
                };
                let result = self.call(&command, args)?;
                Ok(show::pretty(self.out, &result)?)
            }
        }
    }

    /// Open `paths`, files by their full paths, in a new window or the one
    /// named.
    fn open(&mut self, paths: &[OsString], new_window: bool) -> Done {
        let urls: Vec<String> = paths.iter().map(|path| client::address(path)).collect();
        let target = if new_window {
            if self.options.window.is_some() {
                return Err(Stop::Usage("open in a new window, or in -w's".into()));
            }
            let opened = self.call_on("window.new", Value::Null, Target::default())?;
            Target {
                window: Some(show::text(&opened["window"])),
                block: None,
            }
        } else {
            self.target()
        };
        let opened = self.call_on("app.open", json!({"urls": urls}), target)?;
        if self.options.json {
            self.result(&opened)?;
        }
        Ok(())
    }

    fn settings(&mut self, settings: Settings) -> Done {
        match settings {
            Settings::Ls => {
                let listing = self.call("settings.ls", Value::Null)?;
                if self.options.json {
                    return self.result(&listing);
                }
                let rows: Vec<Vec<String>> = listed(&listing)
                    .map(|setting| vec![show::text(&setting["key"]), show::text(&setting["value"])])
                    .collect();
                Ok(show::table(self.out, &["KEY", "VALUE"], &rows)?)
            }
            Settings::Get { key } => {
                let value = self.call("settings.get", json!({"key": key}))?;
                self.result(&value)
            }
            Settings::Set { key, value } => {
                let value = json!({"key": key, "value": typed(None, &value)});
                let now = self.call("settings.set", value)?;
                self.result(&now)
            }
        }
    }

    fn keymap(&mut self) -> Done {
        let keymap = self.call("settings.keymap", Value::Null)?;
        if self.options.json {
            return self.result(&keymap);
        }
        let rows: Vec<Vec<String>> = listed(&keymap)
            .map(|command| {
                let shown: Vec<String> = listed(&command["shown"]).map(show::text).collect();
                vec![
                    shown.join(", "),
                    show::text(&command["command"]),
                    show::text(&command["title"]),
                ]
            })
            .collect();
        Ok(show::table(self.out, &["KEYS", "COMMAND", "TITLE"], &rows)?)
    }

    /// This program's version, and the running Plays', if one is: not
    /// started to say.
    fn version(&mut self) -> Done {
        let running = self
            .socket
            .as_deref()
            .and_then(|path| Client::connect(path).ok())
            .and_then(|mut client| client.hello().ok());
        let version = env!("CARGO_PKG_VERSION");
        if self.options.json {
            let running =
                running.map(|hello| json!({"version": hello["version"], "pid": hello["pid"]}));
            return self.result(&json!({"version": version, "running": running}));
        }
        writeln!(self.out, "plays {version}")?;
        match running {
            Some(hello) => writeln!(
                self.out,
                "running: Plays {}, process {}",
                show::text(&hello["version"]),
                hello["pid"]
            )?,
            None => writeln!(self.out, "running: none")?,
        }
        Ok(())
    }
}

/// The items of `list`, none if it isn't one.
fn listed(list: &Value) -> impl Iterator<Item = &Value> {
    list.as_array().into_iter().flatten()
}

/// `pairs`, each `KEY=VALUE`, as `command`'s arguments: each value as its
/// argument takes it, text as given, numbers and the rest read as JSON.
/// A key given again, or one taking a list, makes a list.
fn arguments(command: &str, pairs: &[String]) -> Result<Value, Stop> {
    if pairs.is_empty() {
        return Ok(Value::Null);
    }
    let spec = api::find(command);
    let mut args = Map::new();
    for pair in pairs {
        let Some((key, text)) = pair.split_once('=') else {
            return Err(Stop::Usage(format!("arguments are KEY=VALUE, not {pair}")));
        };
        let kind = spec
            .and_then(|spec| spec.args.iter().find(|arg| arg.name == key))
            .map(|arg| &arg.kind);
        let value = typed(kind, text);
        match args.get_mut(key) {
            Some(Value::Array(values)) => values.push(value),
            Some(first) => *first = json!([first.take(), value]),
            None if matches!(kind, Some(Kind::Texts)) => {
                args.insert(key.to_owned(), json!([value]));
            }
            None => {
                args.insert(key.to_owned(), value);
            }
        }
    }
    Ok(Value::Object(args))
}

/// `text` as an argument of `kind` takes it: as given for text, else read
/// as JSON if it is some, as given if not.
fn typed(kind: Option<&Kind>, text: &str) -> Value {
    match kind {
        Some(Kind::Text | Kind::Texts | Kind::Choice(_)) => text.into(),
        _ => serde_json::from_str(text).unwrap_or_else(|_| text.into()),
    }
}
