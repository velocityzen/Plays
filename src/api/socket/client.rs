//! The other end of the socket: what the command line, and a Plays handing
//! over the files it was started with, reach the one running with. It
//! asks, waits for each answer, and keeps the events that come meanwhile
//! for `next_event`; or, `Reading`, reads all Plays says on a thread of its
//! own.

use std::collections::VecDeque;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::protocol::{Ask, Heard, Incoming, Refusal, Request};
use crate::api::{self, Target};

/// How long an answer may take: a command runs as a key's does, at once,
/// but the app may be busy.
pub const ANSWER_WAIT: Duration = Duration::from_secs(30);

/// How long a Plays started may take to answer on its socket.
const START_WAIT: Duration = Duration::from_secs(10);

/// How often to look whether it does.
const START_POLL: Duration = Duration::from_millis(50);

/// Why a request didn't do what it asked.
#[derive(Debug)]
pub enum Failure {
    /// No Plays answers.
    Unreachable(io::Error),
    /// Plays closed the connection: it quit, say, before answering, or
    /// while listened to.
    Closed,
    /// The command didn't run, or couldn't do it.
    Command(api::Error),
    /// Plays couldn't read the request, or this its answer.
    BadRequest(String),
}

impl Failure {
    /// The exit status the command line gives it, as Rex's does: those of
    /// `api::Error`, 1 for what Plays couldn't read, 4 for no Plays, or
    /// none any more.
    pub fn exit_status(&self) -> u8 {
        match self {
            Self::Unreachable(_) | Self::Closed => 4,
            Self::Command(err) => err.exit_status(),
            Self::BadRequest(_) => 1,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(err) => write!(f, "can't reach Plays: {err}"),
            Self::Closed => f.write_str("Plays closed the connection"),
            Self::Command(err) => err.fmt(f),
            Self::BadRequest(why) => write!(f, "Plays didn't understand: {why}"),
        }
    }
}

impl std::error::Error for Failure {}

impl From<io::Error> for Failure {
    fn from(err: io::Error) -> Self {
        Self::Unreachable(err)
    }
}

/// Connected to a Plays.
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    /// The last request's id.
    asked: u64,
    /// Events that came while waiting for an answer, by type, with their
    /// payloads.
    events: VecDeque<(String, Value)>,
    /// What's been read of a line not read whole yet, as waiting for it
    /// ran out.
    partial: Vec<u8>,
}

impl Client {
    /// Connect to the Plays answering at `path`.
    pub fn connect(path: &Path) -> io::Result<Self> {
        let writer = UnixStream::connect(path)?;
        Ok(Self {
            reader: BufReader::new(writer.try_clone()?),
            writer,
            asked: 0,
            events: VecDeque::new(),
            partial: Vec::new(),
        })
    }

    /// Ask, and wait for the answer.
    pub fn request(&mut self, ask: Ask) -> Result<Value, Failure> {
        let id = send(&mut self.writer, &mut self.asked, ask)?;
        self.wait(Some(ANSWER_WAIT));
        loop {
            match self.next()? {
                Incoming::Event { kind, payload } => self.events.push_back((kind, payload)),
                Incoming::Answer { id: answered, .. } if answered != id => {
                    log::debug!("an answer to another request: {answered}");
                }
                Incoming::Answer { outcome, .. } => return answered(outcome),
            }
        }
    }

    /// What Plays is: its version, its process id, and this client's
    /// number.
    pub fn hello(&mut self) -> Result<Value, Failure> {
        self.request(Ask::Hello)
    }

    /// Every command, described.
    pub fn commands(&mut self) -> Result<Value, Failure> {
        self.request(Ask::Commands)
    }

    /// The command `command`, described.
    pub fn describe(&mut self, command: &str) -> Result<Value, Failure> {
        self.request(Ask::Describe {
            command: command.to_owned(),
        })
    }

    /// Run `command`, with `args` (null for none), on what `target` names.
    pub fn call(&mut self, command: &str, args: Value, target: Target) -> Result<Value, Failure> {
        self.request(Ask::Call {
            command: command.to_owned(),
            args,
            target,
        })
    }

    /// Hear `heard`'s events from now on, positions too with `positions`.
    pub fn subscribe(&mut self, heard: Heard, positions: bool) -> Result<Value, Failure> {
        self.request(Ask::Subscribe {
            scope: heard,
            positions,
        })
    }

    /// The next event heard, by type, with its payload, waiting for it as
    /// long as it takes.
    pub fn next_event(&mut self) -> Result<(String, Value), Failure> {
        loop {
            if let Some(event) = self.next_event_by(None)? {
                return Ok(event);
            }
        }
    }

    /// The next event heard, waiting for it until `by`, if given; none if
    /// none came by then.
    pub fn next_event_by(
        &mut self,
        by: Option<Instant>,
    ) -> Result<Option<(String, Value)>, Failure> {
        loop {
            if let Some(event) = self.events.pop_front() {
                return Ok(Some(event));
            }
            let left = by.map(|by| by.saturating_duration_since(Instant::now()));
            if left.is_some_and(|left| left.is_zero()) {
                return Ok(None);
            }
            self.wait(left);
            match self.next() {
                Ok(Incoming::Event { kind, payload }) => return Ok(Some((kind, payload))),
                Ok(Incoming::Answer { id, .. }) => log::debug!("an answer to no request: {id}"),
                Err(Failure::Unreachable(err)) if timed_out(&err) => return Ok(None),
                Err(failure) => return Err(failure),
            }
        }
    }

    /// How many events came while waiting for answers, waiting for
    /// `next_event`.
    #[cfg(test)]
    pub fn events_waiting(&self) -> usize {
        self.events.len()
    }

    /// Wait at most `wait` for what Plays sends, or with none as long as it
    /// takes.
    fn wait(&self, wait: Option<Duration>) {
        // Refused once Plays has gone, on macOS, though what it sent before
        // is there to read; reading says it's gone.
        self.reader.get_ref().set_read_timeout(wait).ok();
    }

    /// The next line Plays sends, read. What's read of it before waiting
    /// for it runs out is kept for the next try.
    fn next(&mut self) -> Result<Incoming, Failure> {
        self.reader.read_until(b'\n', &mut self.partial)?;
        incoming(&std::mem::take(&mut self.partial))
    }
}

/// What Plays says on a connection read on a thread of its own.
#[derive(Debug)]
pub enum Said {
    /// An event: its type and payload.
    Event(String, Value),
    /// The answer to the request with this id: what it comes to.
    Answer(Value, Result<Value, Failure>),
}

/// A connection to a Plays read on a thread of its own, for a client
/// waiting on other things too: what Plays says goes to `heard` as it
/// comes, answers to its requests too, until the connection closes or
/// `heard` wants no more.
pub struct Reading {
    writer: UnixStream,
    /// The last request's id.
    asked: u64,
}

impl Reading {
    /// Connect to the Plays answering at `path`.
    pub fn connect(
        path: &Path,
        mut heard: impl FnMut(Result<Said, Failure>) -> bool + Send + 'static,
    ) -> io::Result<Self> {
        let writer = UnixStream::connect(path)?;
        let mut reader = BufReader::new(writer.try_clone()?);
        thread::spawn(move || {
            loop {
                let mut line = Vec::new();
                let said = match reader.read_until(b'\n', &mut line) {
                    Ok(_) => incoming(&line).map(|incoming| match incoming {
                        Incoming::Event { kind, payload } => Said::Event(kind, payload),
                        Incoming::Answer { id, outcome } => Said::Answer(id, answered(outcome)),
                    }),
                    Err(err) => Err(err.into()),
                };
                let over = matches!(said, Err(Failure::Closed | Failure::Unreachable(_)));
                if !heard(said) || over {
                    return;
                }
            }
        });
        Ok(Self { writer, asked: 0 })
    }

    /// Ask what Plays is: the id its answer comes with, after every event
    /// told before.
    pub fn hello(&mut self) -> Result<Value, Failure> {
        send(&mut self.writer, &mut self.asked, Ask::Hello)
    }

    /// Hear `heard`'s events from now on, positions too with `positions`:
    /// the id its answer comes with.
    pub fn subscribe(&mut self, heard: Heard, positions: bool) -> Result<Value, Failure> {
        let ask = Ask::Subscribe {
            scope: heard,
            positions,
        };
        send(&mut self.writer, &mut self.asked, ask)
    }
}

impl Drop for Reading {
    /// Closed both ways, so the thread reading it ends.
    fn drop(&mut self) {
        self.writer.shutdown(Shutdown::Both).ok();
    }
}

/// Send `ask` on `writer`, the request after `asked`: its id.
fn send(writer: &mut UnixStream, asked: &mut u64, ask: Ask) -> Result<Value, Failure> {
    *asked += 1;
    let id = Value::from(*asked);
    let request = Request {
        id: id.clone(),
        ask,
    };
    let line =
        serde_json::to_string(&request).map_err(|err| Failure::BadRequest(err.to_string()))?;
    writeln!(writer, "{line}")?;
    Ok(id)
}

/// What a request comes to, answered with `outcome`.
fn answered(outcome: Result<Value, Refusal>) -> Result<Value, Failure> {
    outcome.map_err(|refusal| match refusal {
        Refusal::Command(err) => Failure::Command(err),
        Refusal::BadRequest(why) => Failure::BadRequest(why),
    })
}

/// `line`, read whole, as Plays sent it: none, if it closed the connection
/// before ending it.
fn incoming(line: &[u8]) -> Result<Incoming, Failure> {
    if line.last() != Some(&b'\n') {
        return Err(Failure::Closed);
    }
    Incoming::read(&String::from_utf8_lossy(line)).map_err(Failure::BadRequest)
}

/// Whether `err` says waiting for Plays ran out.
fn timed_out(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Hand `args`, the files, folders and URLs a Plays was started with, to
/// the one answering at `path`, which opens them and comes to the front;
/// with none, it comes to the front.
pub fn hand_over(path: &Path, args: &[OsString]) -> Result<(), Failure> {
    let urls = args.iter().map(|arg| address(arg)).collect();
    Client::connect(path)?.request(Ask::HandOver { urls })?;
    Ok(())
}

/// What opens `arg` in a Plays that may have started elsewhere: a URL as
/// given, which it opens or says why it can't, or a file by its full path.
pub fn address(arg: &OsStr) -> String {
    let text = arg.to_string_lossy();
    if text.contains("://") {
        return text.into_owned();
    }
    let path = Path::new(arg);
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_owned())
        .display()
        .to_string()
}

/// Connect to the Plays answering at `path`; with none answering, start
/// one with `start`, if given, and wait for it to answer, for at most
/// `START_WAIT`.
pub fn connect_or_start(
    path: &Path,
    start: Option<&dyn Fn() -> io::Result<()>>,
) -> Result<Client, Failure> {
    let err = match Client::connect(path) {
        Ok(client) => return Ok(client),
        Err(err) => err,
    };
    let Some(start) = start.filter(|_| none_answers(&err)) else {
        return Err(err.into());
    };
    start()?;
    let started = Instant::now();
    loop {
        thread::sleep(START_POLL);
        match Client::connect(path) {
            Ok(client) => return Ok(client),
            Err(err) if started.elapsed() >= START_WAIT => return Err(err.into()),
            Err(_) => {}
        }
    }
}

/// Whether `err`, connecting, says no Plays answers there: there's no
/// socket, or one left by a Plays that's gone.
pub fn none_answers(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
    )
}

/// Start Plays, this program, apart from this process: it outlives it,
/// out of the terminal's way, its output going nowhere. By its own path,
/// not the link a terminal ran it by, so on macOS it starts as the app
/// it's in. Not in tests, which would start themselves.
pub fn start_plays() -> io::Result<()> {
    Command::new(std::env::current_exe()?.canonicalize()?)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
}
