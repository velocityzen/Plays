//! What the tests share: folders of their own, bytes like a binary's, and
//! an HTTP server on this machine.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use url::Url;

use crate::package::remove;

/// A folder of a test's own, gone once it's dropped.
pub(crate) struct Scratch(pub PathBuf);

impl Scratch {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "updater-tests-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        remove(&path);
        fs::create_dir_all(&path).unwrap();
        // Canonical, as `current_exe` gives it: /var is /private/var on macOS.
        Self(path.canonicalize().unwrap())
    }

    pub(crate) fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    /// The names in it, sorted.
    pub(crate) fn names(&self, folder: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.join(folder))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        remove(&self.0);
    }
}

/// What a binary might be, as far as compressing it goes: bytes that
/// hardly do. The same for the same `seed`.
pub(crate) fn binary(seed: u8, len: usize) -> Vec<u8> {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64 ^ u64::from(seed);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

/// How the server answers a path.
#[derive(Clone, Debug)]
pub(crate) enum Answer {
    Ok(Vec<u8>),
    Status(u16),
    /// 302, to the path on this server, as release downloads on GitHub are
    /// sent on to where the files are.
    Redirect(String),
}

/// An HTTP server on this machine: each path answered as it's set, 404
/// if it isn't, and each path asked for noted.
pub(crate) struct Server {
    address: SocketAddr,
    answers: Arc<Mutex<HashMap<String, Answer>>>,
    asked: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub(crate) fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = Self {
            address,
            answers: Arc::default(),
            asked: Arc::default(),
            stop: Arc::default(),
        };
        let (answers, asked, stop) = (
            server.answers.clone(),
            server.asked.clone(),
            server.stop.clone(),
        );
        thread::spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let (answers, asked) = (answers.clone(), asked.clone());
                thread::spawn(move || answer(stream, &answers, &asked));
            }
        });
        server
    }

    pub(crate) fn set(&self, path: &str, answer: Answer) {
        self.answers.lock().unwrap().insert(path.to_owned(), answer);
    }

    pub(crate) fn url(&self, path: &str) -> Url {
        Url::parse(&format!("http://{}/{path}", self.address)).unwrap()
    }

    /// The paths asked for, in order.
    pub(crate) fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Waking it, to stop.
        let _ = TcpStream::connect(self.address);
    }
}

fn answer(stream: TcpStream, answers: &Mutex<HashMap<String, Answer>>, asked: &Mutex<Vec<String>>) {
    let mut request = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    if request.read_line(&mut line).is_err() {
        return;
    }
    let path = line
        .split(' ')
        .nth(1)
        .unwrap_or("/")
        .trim_start_matches('/')
        .to_owned();
    loop {
        let mut header = String::new();
        if request.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
            break;
        }
    }
    asked.lock().unwrap().push(path.clone());
    let answer = answers
        .lock()
        .unwrap()
        .get(&path)
        .cloned()
        .unwrap_or(Answer::Status(404));
    let (status, body, location) = match answer {
        Answer::Ok(body) => (200, body, String::new()),
        Answer::Status(status) => (status, Vec::new(), String::new()),
        Answer::Redirect(to) => {
            let here = stream.local_addr().unwrap();
            (302, Vec::new(), format!("Location: http://{here}/{to}\r\n"))
        }
    };
    let mut stream = stream;
    // The client may stop reading before it's all sent.
    let _ = write!(
        stream,
        "HTTP/1.1 {status} Answer\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
}
