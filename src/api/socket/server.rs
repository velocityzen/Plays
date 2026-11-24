//! Answering on the socket. A thread takes each client that connects, and
//! two more each read what it asks and write what it's told; the app
//! answers on the UI thread, as a key's command runs, in the order asked.
//! Events a client hears go out as they're told, those told as a request
//! runs before its answer. A client that stops reading, with `OUTBOX`
//! lines waiting for it, is cut off rather than kept in memory. As the
//! app quits, each client is sent what it was told, the windows closing
//! and `app.quit` among it, before its connection closes (`end_on_quit`).
//!
//! With control from the command line off in Settings, only files handed
//! over are taken: all else is refused (`allowed`), and turning it off
//! cuts every client off. A Plays not listening, as another answers there
//! or the socket can't be taken, says why in Settings (`NotListening`),
//! and tries again as control is turned on.

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::poll_fn;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui::{App, Global, Task};
use serde_json::{Value, json};

use super::protocol::{self, Ask, Heard, Request};
use super::{Claimed, Listener};
use crate::api::events::{self, Event, Scope};
use crate::api::{self, Error, Invocation, Source, Spec, Target, target};
use crate::settings::Settings;

/// How many lines may wait for a client before it's cut off.
const OUTBOX: usize = 1024;

/// The longest line a client may send, in bytes; one longer cuts it off.
pub(super) const LONGEST: u64 = 1 << 20;

/// How long to wait before taking the next client, as taking the last
/// failed: with no file descriptors left, say.
const ACCEPT_PAUSE: Duration = Duration::from_millis(100);

/// How often the app, quitting, looks whether each client has been sent
/// all it was told.
const WRITTEN_POLL: Duration = Duration::from_millis(2);

/// A client's number, told with `app.client_connected` and `app.client_left`.
type ClientId = u64;

/// The clients connected, by number.
type Clients = Rc<RefCell<HashMap<ClientId, Client>>>;

/// Why this Plays doesn't listen on its socket, while it doesn't: another
/// answers there, or it can't be taken. For Settings to say.
#[derive(Default)]
pub struct NotListening(pub Option<String>);

impl Global for NotListening {}

/// The socket, as the app answers on it.
struct Socket {
    path: PathBuf,
    /// What the threads tell the app.
    messages: UnboundedSender<Message>,
    /// The last client's number.
    last: Arc<AtomicU64>,
    /// While it's taken: the thread taking clients.
    taken: Option<Taking>,
}

/// The thread taking clients, and what stops it.
struct Taking {
    stopped: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

impl Socket {
    /// Take the socket, unless another Plays answers there, and each
    /// client connecting: why not, if it isn't taken.
    fn claim(&mut self) -> Option<String> {
        let path = self.path.display();
        let why = match super::claim(&self.path) {
            Ok(Claimed::Ours(listener)) => return self.take(listener),
            Ok(Claimed::Running) => "Another Plays answers it.".to_owned(),
            Err(err) => format!("Its socket, {path}, can't be taken: {err}."),
        };
        log::warn!("not listening on {path}: {why}");
        Some(why)
    }

    /// Take each client connecting on `listener`, on a thread of its own:
    /// why not, if it can't.
    fn take(&mut self, listener: Listener) -> Option<String> {
        let stopped = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stopped);
        let (last, messages) = (Arc::clone(&self.last), self.messages.clone());
        let taking = thread::Builder::new()
            .name("plays-socket".into())
            .spawn(move || accept(&listener.listener, &stopping, &last, &messages));
        match taking {
            Ok(thread) => {
                self.taken = Some(Taking { stopped, thread });
                None
            }
            Err(err) => {
                log::warn!("can't answer on the socket: {err}");
                super::remove(&self.path).ok();
                Some(format!("It can't answer on its socket: {err}."))
            }
        }
    }

    /// Stop taking clients and remove the socket, if it's taken: the
    /// thread that took them, which ends once woken. With the lock held,
    /// so another Plays taking the socket as this one lets go of it keeps
    /// it.
    fn close(&mut self) -> Option<JoinHandle<()>> {
        let Taking { stopped, thread } = self.taken.take()?;
        let looking = super::locked(&self.path);
        stopped.store(true, Ordering::SeqCst);
        // It waits for a client: one connecting wakes it to stop.
        if let Err(err) = UnixStream::connect(&self.path) {
            log::debug!("can't wake the thread taking socket clients: {err}");
        }
        match looking {
            Ok(_) => drop(super::remove(&self.path)),
            Err(err) => log::debug!("leaving the socket, as its lock can't be taken: {err}"),
        }
        Some(thread)
    }
}

/// What the threads tell the app.
enum Message {
    /// A client connected, how to write to it, and the thread writing.
    Connected(ClientId, Out, JoinHandle<()>),
    /// A line from a client.
    Line(ClientId, String),
    /// A client went, or was cut off.
    Left(ClientId),
}

/// What goes out to a client: a line, or the end, once what's before it
/// is written, of its connection.
#[derive(Debug, PartialEq)]
enum Outgoing {
    Line(String),
    End,
}

/// Writing to a client.
#[derive(Clone)]
struct Out {
    lines: SyncSender<Outgoing>,
    stream: Arc<UnixStream>,
}

impl Out {
    /// Send `line`.
    fn send(&self, line: String) {
        self.put(Outgoing::Line(line));
    }

    /// Close the connection once what was sent is written.
    fn end(&self) {
        self.put(Outgoing::End);
    }

    /// Send `outgoing`, or cut the client off if too much waits for it.
    fn put(&self, outgoing: Outgoing) {
        if let Err(TrySendError::Full(_)) = self.lines.try_send(outgoing) {
            log::warn!("cutting off a socket client that isn't reading");
            self.stream.shutdown(Shutdown::Both).ok();
        }
    }
}

/// A client, as the app sees it.
struct Client {
    out: Out,
    /// The events it hears, while it does.
    hearing: Option<Hearing>,
    /// The thread writing to it, which ends once its connection closes.
    writing: JoinHandle<()>,
}

impl Client {
    /// Send what it was told and is still waiting to be, then close its
    /// connection; what's handed back finishes once that's written.
    fn end(self) -> JoinHandle<()> {
        if let Some(hearing) = &self.hearing {
            hearing.send_waiting(&self.out);
        }
        self.out.end();
        self.writing
    }
}

/// The events a client hears, and the task sending each as it's told.
struct Hearing {
    told: Rc<RefCell<UnboundedReceiver<Event>>>,
    _sending: Task<()>,
}

impl Hearing {
    /// Send those told and still waiting to be, as before an answer: the
    /// events told as its request ran, such as its `app.command_run`, come
    /// first.
    fn send_waiting(&self, out: &Out) {
        while let Ok(event) = self.told.borrow_mut().try_recv() {
            tell(out, &event);
        }
    }
}

/// Send `event` to a client.
fn tell(out: &Out, event: &Event) {
    match serde_json::to_string(event) {
        Ok(line) => out.send(line),
        Err(err) => log::warn!("can't tell {}: {err}", event.kind),
    }
}

/// Answer what clients ask on the socket at `path`, taken as Plays
/// started (`listener`), else now: with control from the command line off,
/// only files handed over. Removed as the app quits.
pub fn serve(path: PathBuf, listener: Option<Listener>, cx: &mut App) {
    let (messages, mut received) = unbounded();
    let mut socket = Socket {
        path,
        messages,
        last: Arc::default(),
        taken: None,
    };
    let why = match listener {
        Some(listener) => socket.take(listener),
        None => socket.claim(),
    };
    cx.set_global(NotListening(why));
    let socket = Rc::new(RefCell::new(socket));
    let clients = Clients::default();
    follow_control(&socket, &clients, cx);
    end_on_quit(socket, &clients, cx);
    cx.spawn(async move |cx| {
        while let Some(message) = received.next().await {
            let handled = cx.update(|cx| handle(message, &mut clients.borrow_mut(), cx));
            if handled.is_err() {
                break;
            }
        }
    })
    .detach();
}

/// As the app quits, tell those listening the windows closing and `app.quit`,
/// send each client what it was told, as nothing else runs then, and
/// close its connection; remove the socket, and wait, as long as quitting
/// does, for what was sent to be written.
fn end_on_quit(socket: Rc<RefCell<Socket>>, clients: &Clients, cx: &mut App) {
    let clients = Rc::clone(clients);
    cx.on_app_quit(move |cx| {
        events::quit(cx);
        let writing: Vec<JoinHandle<()>> = clients
            .borrow_mut()
            .drain()
            .map(|(_, client)| client.end())
            .collect();
        socket.borrow_mut().close();
        let executor = cx.background_executor().clone();
        async move {
            while !writing.iter().all(JoinHandle::is_finished) {
                executor.timer(WRITTEN_POLL).await;
            }
        }
    })
    .detach();
}

/// Whether Settings has control from the command line on.
fn allowed(cx: &App) -> bool {
    cx.try_global::<Settings>()
        .is_some_and(|settings| settings.command_line_control)
}

/// As Settings turns control from the command line off, let go of every
/// client: each hears no more and is told nothing more, its connection
/// closing once what it was sent is written. The socket stays, for files
/// handed over. As it turns it on, take the socket, if this Plays doesn't
/// listen on it.
fn follow_control(socket: &Rc<RefCell<Socket>>, clients: &Clients, cx: &mut App) {
    let socket = Rc::clone(socket);
    let clients = Rc::clone(clients);
    let mut was = allowed(cx);
    cx.observe_global::<Settings>(move |cx| {
        let now = allowed(cx);
        if !was && now && socket.borrow().taken.is_none() {
            let why = socket.borrow_mut().claim();
            cx.set_global(NotListening(why));
        }
        if was && !now {
            // Once what it's answering, if anything, is answered.
            let clients = Rc::clone(&clients);
            cx.defer(move |cx| {
                let gone: Vec<ClientId> = clients.borrow_mut().drain().map(|(id, _)| id).collect();
                for client in gone {
                    events::client_left(client, cx);
                }
            });
        }
        was = now;
    })
    .detach();
}

/// Take each client that connects, numbering them on from `last`, until
/// `stopped`, or the app stops listening.
fn accept(
    listener: &UnixListener,
    stopped: &AtomicBool,
    last: &AtomicU64,
    messages: &UnboundedSender<Message>,
) {
    for stream in listener.incoming() {
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        let stream = match stream {
            Ok(stream) => stream,
            Err(err) => {
                log::warn!("can't take a socket client: {err}");
                thread::sleep(ACCEPT_PAUSE);
                continue;
            }
        };
        let client = last.fetch_add(1, Ordering::SeqCst) + 1;
        match connect(client, stream, messages) {
            Ok(()) => {}
            Err(err) if messages.is_closed() => {
                log::debug!("the app has stopped answering on the socket: {err}");
                return;
            }
            Err(err) => log::warn!("can't answer a socket client: {err}"),
        }
    }
}

/// Start reading what `client` asks and writing what it's told.
fn connect(
    client: ClientId,
    stream: UnixStream,
    messages: &UnboundedSender<Message>,
) -> io::Result<()> {
    let stream = Arc::new(stream);
    let (lines, outgoing) = mpsc::sync_channel(OUTBOX);
    let writing = Arc::clone(&stream);
    let writing = thread::Builder::new()
        .name("plays-socket-out".into())
        .spawn(move || write(&writing, &outgoing))?;
    // Before anything it asks.
    let out = Out {
        lines,
        stream: Arc::clone(&stream),
    };
    messages
        .unbounded_send(Message::Connected(client, out, writing))
        .map_err(|_| io::Error::other("the app isn't answering"))?;
    let messages = messages.clone();
    thread::Builder::new()
        .name("plays-socket-in".into())
        .spawn(move || read(client, &stream, &messages))?;
    Ok(())
}

/// Send what `client` asks, a line at a time, until it goes.
fn read(client: ClientId, stream: &UnixStream, messages: &UnboundedSender<Message>) {
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        match (&mut reader).take(LONGEST).read_line(&mut line) {
            Ok(0) => break,
            Ok(read) if read as u64 == LONGEST && !line.ends_with('\n') => {
                log::warn!("cutting off a socket client that sent too long a line");
                break;
            }
            Ok(_) => {
                let line = line.trim_end_matches(['\n', '\r']);
                if !line.trim().is_empty()
                    && messages
                        .unbounded_send(Message::Line(client, line.to_owned()))
                        .is_err()
                {
                    break;
                }
            }
            Err(err) => {
                log::debug!("socket client {client} went: {err}");
                break;
            }
        }
    }
    stream.shutdown(Shutdown::Both).ok();
    messages.unbounded_send(Message::Left(client)).ok();
}

/// Write each line sent to the client, until it goes, is cut off, or its
/// connection's end is sent; then close it.
fn write(stream: &UnixStream, outgoing: &Receiver<Outgoing>) {
    let mut out = BufWriter::new(stream);
    'writing: while let Ok(first) = outgoing.recv() {
        // Those waiting too, before flushing.
        for next in std::iter::once(first).chain(outgoing.try_iter()) {
            let Outgoing::Line(line) = next else {
                out.flush().ok();
                break 'writing;
            };
            if writeln!(out, "{line}").is_err() {
                break 'writing;
            }
        }
        if out.flush().is_err() {
            break;
        }
    }
    stream.shutdown(Shutdown::Both).ok();
}

/// Do what a thread tells.
fn handle(message: Message, clients: &mut HashMap<ClientId, Client>, cx: &mut App) {
    match message {
        Message::Connected(client, out, writing) => {
            let connected = Client {
                out,
                hearing: None,
                writing,
            };
            clients.insert(client, connected);
            events::client_connected(client, cx);
        }
        Message::Line(id, line) => {
            let Some(client) = clients.get_mut(&id) else {
                return;
            };
            let answer = match protocol::read(&line) {
                Ok(Request { id: asked, ask }) => {
                    protocol::answer(&asked, answer(id, ask, client, cx))
                }
                Err((asked, why)) => protocol::bad_request(&asked, &why),
            };
            if let Some(hearing) = &client.hearing {
                hearing.send_waiting(&client.out);
            }
            client.out.send(answer.to_string());
        }
        Message::Left(client) => {
            if clients.remove(&client).is_some() {
                events::client_left(client, cx);
            }
        }
    }
}

/// What `ask` comes back with, for `client`, numbered `id`.
fn answer(id: ClientId, ask: Ask, client: &mut Client, cx: &mut App) -> Result<Value, Error> {
    if !matches!(ask, Ask::HandOver { .. }) && !allowed(cx) {
        return Err(Error::command_line_off());
    }
    match ask {
        Ask::Hello => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "client_id": id,
        })),
        Ask::Call {
            command,
            args,
            target,
        } => {
            let query = api::find(&command).is_some_and(|spec| spec.query);
            let invocation = Invocation {
                command,
                args,
                target,
                source: Source::Cli,
            };
            let done = api::run(invocation, cx);
            if !query {
                crate::app::refresh_menus(cx);
            }
            done
        }
        Ask::Commands => Ok(api::all().map(Spec::describe).collect()),
        Ask::Describe { command } => api::find(&command)
            .map(Spec::describe)
            .ok_or(Error::NoSuchCommand { command }),
        Ask::Subscribe { scope, positions } => subscribe(&scope, positions, client, cx),
        Ask::Unsubscribe => {
            client.hearing = None;
            Ok(Value::Null)
        }
        Ask::HandOver { urls } => hand_over(urls, cx),
    }
}

/// Send `client` the events `heard` names from now on, in place of those
/// it heard: positions too with `positions`. Hands back the id of the
/// window named, as names change.
fn subscribe(
    heard: &Heard,
    positions: bool,
    client: &mut Client,
    cx: &mut App,
) -> Result<Value, Error> {
    let (scope, answer) = match heard {
        Heard::App => (Scope::App, Value::Null),
        Heard::All => (Scope::All, Value::Null),
        Heard::Window(name) => {
            let named = Target {
                window: Some(name.clone()),
                block: None,
            };
            let id = target::read(target::window(&named, cx)?, cx)?.id();
            (Scope::Window(id), json!({"window": id.to_string()}))
        }
    };
    let told = Rc::new(RefCell::new(events::listen(scope, positions, cx)));
    let out = client.out.clone();
    let heard = Rc::clone(&told);
    let sending = cx.spawn(async move |_| {
        while let Some(event) = poll_fn(|cx| heard.borrow_mut().poll_next_unpin(cx)).await {
            tell(&out, &event);
        }
    });
    client.hearing = Some(Hearing {
        told,
        _sending: sending,
    });
    Ok(answer)
}

/// Open `urls`, handed over by a Plays started with them, as files opened
/// from Finder are, and come to the front, in front of the terminal they
/// may have come from; with none, bring the window in front forward, or
/// open one.
fn hand_over(urls: Vec<String>, cx: &mut App) -> Result<Value, Error> {
    let invocation = if !urls.is_empty() {
        Invocation::new("app.open").with_args(json!({"urls": urls}))
    } else if let Some(window) = target::front_window(cx) {
        Invocation::new("window.focus").in_window(target::read(window, cx)?.id().to_string())
    } else {
        Invocation::new("window.new")
    };
    let done = api::run(invocation.from(Source::Cli), cx);
    crate::app::refresh_menus(cx);
    cx.activate(true);
    done
}

#[cfg(test)]
#[path = "tests/server.rs"]
mod tests;
