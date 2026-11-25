//! What comes to a script as it runs, in one channel, in the order it
//! comes, kept until the script takes it as it waits or listens: events,
//! and what its programs write and how they end (`exec`).
//!
//! Events come on a connection of their own, read on a thread of its own
//! and subscribed at the script's first call or wait, so calls and events
//! don't wait on each other: everything's, or with `-w` that window's,
//! where files are once a wait or a handler is for that, and the cues a
//! wait or a handler is for, in whatever window. A wait hears what
//! happens from the start of the last call before it, in a handler the
//! last in it; with none, from its own start, or in a handler from its
//! event; or from a mark it's given. Each event once, in order, those a
//! wait passes over gone, for handlers too. A script with no handlers
//! passes over those that come as it runs on.

use std::collections::VecDeque;
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use serde_json::Value;

use super::exec::Ran;
use crate::api::socket::Heard;
use crate::api::socket::client::{ANSWER_WAIT, Failure, Reading, Said};

/// Something come to a script.
pub enum Came {
    /// What Plays said on the events' connection, or why it says no more.
    Said(Result<Said, Failure>),
    /// What a program did.
    Ran(Ran),
}

/// What comes next as a script listens.
pub enum Next {
    /// An event: its type and payload.
    Event(String, Value),
    Ran(Ran),
}

/// The answer to a request: its id, and what it comes to.
struct Answered {
    id: Value,
    answer: Result<Value, Failure>,
}

/// What's come to a script, and what's to come.
pub struct Inbox {
    socket: Option<PathBuf>,
    /// Whose events: everything's, or a window's.
    heard: Heard,
    connection: Option<Reading>,
    /// Whether the connection closed, so nothing more comes on it.
    closed: bool,
    /// Whether where files are is heard too.
    positions: bool,
    /// How many events have been taken, or passed over: the next's place
    /// among all those heard, from 0.
    taken: u64,
    /// The place of the first event told after the last call began: a
    /// wait passes over those before it.
    call_began: u64,
    sender: Sender<Came>,
    receiver: Receiver<Came>,
    /// Events come, not taken yet, by type, with their payloads.
    events: VecDeque<(String, Value)>,
    /// What programs did, not taken yet.
    ran: VecDeque<Ran>,
}

impl Inbox {
    /// For a script hearing `heard`'s events from the Plays answering at
    /// `socket`: nothing come yet.
    pub fn new(socket: Option<PathBuf>, heard: Heard) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            socket,
            heard,
            connection: None,
            closed: false,
            positions: false,
            taken: 0,
            call_began: 0,
            sender,
            receiver,
            events: VecDeque::new(),
            ran: VecDeque::new(),
        }
    }

    /// What a program's threads tell what it does with.
    pub fn sender(&self) -> Sender<Came> {
        self.sender.clone()
    }

    /// As a call begins: a wait after it hears what happens from now on,
    /// and nothing told before. Failing, the call goes on: a wait after
    /// says why.
    pub fn hear_from_now(&mut self) {
        match self.mark() {
            Ok(now) => self.call_began = now,
            Err(failure) => log::debug!("can't hear events: {failure}"),
        }
    }

    /// The place of the first event told from now on, once all told
    /// before has come: a wait given it hears from then.
    pub fn mark(&mut self) -> Result<u64, Failure> {
        self.connect()?;
        let id = self.connection()?.hello()?;
        self.answer(&id)?;
        Ok(self.taken + self.events.len() as u64)
    }

    /// Hear where files are as they play too, from now on.
    pub fn hear_positions(&mut self) -> Result<(), Failure> {
        if !self.positions {
            self.connect()?;
            let heard = self.heard.clone();
            let id = self.connection()?.subscribe(heard, true)?;
            self.answer(&id)?;
            self.positions = true;
        }
        Ok(())
    }

    /// Hear the cue `name` from now on: its block reaching a time or a
    /// frame, once. The block's id.
    pub fn cue(&mut self, name: &str) -> Result<String, Failure> {
        self.connect()?;
        let id = self.connection()?.cue(name)?;
        let set = self.answer(&id)?;
        Ok(set["block"].as_str().unwrap_or_default().to_owned())
    }

    /// The connection events come on, subscribed, unless it is already.
    fn connect(&mut self) -> Result<(), Failure> {
        if self.connection.is_some() {
            return Ok(());
        }
        let Some(path) = self.socket.as_deref() else {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no socket").into());
        };
        let sender = self.sender.clone();
        let connection = Reading::connect(path, move |said| sender.send(Came::Said(said)).is_ok())?;
        let subscribed = self
            .connection
            .insert(connection)
            .subscribe(self.heard.clone(), false);
        let answered = subscribed.and_then(|id| self.answer(&id));
        if answered.is_err() {
            self.connection = None;
        }
        answered.map(drop)
    }

    fn connection(&mut self) -> Result<&mut Reading, Failure> {
        self.connection.as_mut().ok_or(Failure::Closed)
    }

    /// The answer to the request with `id`, what comes meanwhile kept.
    fn answer(&mut self, id: &Value) -> Result<Value, Failure> {
        let by = Instant::now() + ANSWER_WAIT;
        loop {
            if self.closed {
                return Err(Failure::Closed);
            }
            let Some(came) = self.receive(Some(by)) else {
                return Err(Failure::TimedOut);
            };
            match self.keep(came)? {
                Some(answered) if answered.id == *id => return answered.answer,
                Some(answered) => log::debug!("an answer to another request: {}", answered.id),
                None => {}
            }
        }
    }

    /// The next event heard, for a wait, by its type, with its payload,
    /// waiting for it until `by`, if given: none by then.
    pub fn next(&mut self, by: Option<Instant>) -> Result<Option<(String, Value)>, Failure> {
        self.connect()?;
        loop {
            if let Some(event) = self.events.pop_front() {
                self.taken += 1;
                return Ok(Some(event));
            }
            if self.closed {
                return Err(Failure::Closed);
            }
            let Some(came) = self.receive(by) else {
                return Ok(None);
            };
            if let Some(answered) = self.keep(came)? {
                log::debug!("an answer to no request: {}", answered.id);
            }
        }
    }

    /// Pass over those told before `mark`, or else before the last call
    /// began, heard already.
    pub fn pass_over_told(&mut self, mark: Option<u64>) {
        let from = mark.unwrap_or(self.call_began);
        while self.taken < from && self.events.pop_front().is_some() {
            self.taken += 1;
        }
    }

    /// The next thing a program did, waiting for it until `by`, if given:
    /// none by then. Events come meanwhile are kept.
    pub fn next_ran(&mut self, by: Option<Instant>) -> Option<Ran> {
        loop {
            if let Some(ran) = self.ran.pop_front() {
                return Some(ran);
            }
            let came = self.receive(by)?;
            match self.keep(came) {
                Ok(Some(answered)) => log::debug!("an answer to no request: {}", answered.id),
                Ok(None) => {}
                Err(failure) => log::debug!("can't hear events: {failure}"),
            }
        }
    }

    /// The next thing come as the script listens, waiting for it until
    /// `by`, if given: none by then. Events are for `handlers`, if any:
    /// with none, those come are passed over. A wait in a handler hears
    /// from its event on.
    pub fn next_listening(
        &mut self,
        by: Option<Instant>,
        handlers: bool,
    ) -> Result<Option<Next>, Failure> {
        if handlers {
            self.connect()?;
        }
        self.call_began = self.taken;
        loop {
            if !handlers {
                self.taken += self.events.len() as u64;
                self.events.clear();
            }
            if let Some(ran) = self.ran.pop_front() {
                return Ok(Some(Next::Ran(ran)));
            }
            if let Some((kind, payload)) = self.events.pop_front() {
                self.taken += 1;
                return Ok(Some(Next::Event(kind, payload)));
            }
            if handlers && self.closed {
                return Err(Failure::Closed);
            }
            let Some(came) = self.receive(by) else {
                return Ok(None);
            };
            match self.keep(came) {
                Ok(Some(answered)) => log::debug!("an answer to no request: {}", answered.id),
                Ok(None) => {}
                Err(failure) if handlers => return Err(failure),
                Err(failure) => log::debug!("can't hear events: {failure}"),
            }
        }
    }

    /// The next thing come, waiting for it until `by`, if given: none by
    /// then, though what came before is.
    fn receive(&self, by: Option<Instant>) -> Option<Came> {
        match by {
            Some(by) => {
                let left = by.saturating_duration_since(Instant::now());
                self.receiver.recv_timeout(left).ok()
            }
            None => self.receiver.recv().ok(),
        }
    }

    /// Keep `came` until it's taken: an answer, though, by the id of its
    /// request, and why the connection says no more, if it doesn't.
    fn keep(&mut self, came: Came) -> Result<Option<Answered>, Failure> {
        match came {
            Came::Ran(ran) => self.ran.push_back(ran),
            Came::Said(Ok(Said::Event(kind, payload))) => self.events.push_back((kind, payload)),
            Came::Said(Ok(Said::Answer(id, answer))) => return Ok(Some(Answered { id, answer })),
            Came::Said(Err(failure)) => {
                self.closed |= matches!(failure, Failure::Closed | Failure::Unreachable(_));
                return Err(failure);
            }
        }
        Ok(None)
    }
}
