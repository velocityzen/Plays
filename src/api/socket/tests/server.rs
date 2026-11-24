use super::*;

#[test]
fn a_client_with_too_much_waiting_for_it_is_cut_off() {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let (lines, waiting) = mpsc::sync_channel(1);
    let out = Out {
        lines,
        stream: Arc::new(ours),
    };
    theirs.set_nonblocking(true).unwrap();
    let mut read = String::new();
    let open = |read: &mut String| BufReader::new(&theirs).read_line(read);

    out.send("one".into());
    let err = open(&mut read).expect_err("nothing written yet");
    assert_eq!(err.kind(), io::ErrorKind::WouldBlock, "still connected");
    out.send("two".into());
    assert_eq!(
        waiting.try_iter().collect::<Vec<_>>(),
        [Outgoing::Line("one".into())]
    );
    assert_eq!(open(&mut read).unwrap(), 0, "cut off");
}

#[test]
fn a_client_sent_the_end_is_written_what_came_before_then_its_connection_closes() {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let (lines, outgoing) = mpsc::sync_channel(OUTBOX);
    let out = Out {
        lines,
        stream: Arc::new(ours),
    };
    out.send("one".into());
    out.send("two".into());
    out.end();
    out.send("after".into());
    let stream = Arc::clone(&out.stream);
    let writing = thread::spawn(move || write(&stream, &outgoing));
    writing.join().unwrap();

    let mut written = String::new();
    BufReader::new(&theirs)
        .read_to_string(&mut written)
        .unwrap();
    assert_eq!(written, "one\ntwo\n", "then the end, and nothing after");
    assert!(
        out.lines.try_send(Outgoing::End).is_err(),
        "no longer written"
    );
}

/// Closing waits for another Plays looking at the socket, so one taking
/// it as this one lets go of it keeps it.
#[test]
fn closing_the_socket_waits_for_a_plays_looking_at_it() {
    let path = crate::api::socket::tests::socket("close-turn");
    let (messages, _received) = unbounded();
    let mut socket = Socket {
        path: path.clone(),
        messages,
        last: Arc::default(),
        taken: None,
    };
    assert_eq!(socket.claim(), None, "taken");
    let lock = std::fs::File::open(path.with_file_name(super::super::LOCK)).unwrap();
    lock.lock().unwrap();
    let closing = thread::spawn(move || socket.close().map(JoinHandle::join));
    thread::sleep(Duration::from_millis(100));
    assert!(!closing.is_finished(), "it waits its turn");
    assert!(path.exists(), "the socket left till then");
    lock.unlock().unwrap();
    assert!(closing.join().unwrap().is_some_and(|ended| ended.is_ok()));
    assert!(!path.exists(), "then removed");
}

#[test]
fn closing_the_socket_stops_the_thread_taking_clients() {
    let path = crate::api::socket::tests::socket("close");
    let (messages, _received) = unbounded();
    let mut socket = Socket {
        path: path.clone(),
        messages,
        last: Arc::default(),
        taken: None,
    };
    assert_eq!(socket.claim(), None, "taken");
    // Not connecting, which would wake the thread itself.
    assert!(path.exists(), "taken");
    let taking = socket.close().expect("it was taken");
    let ended = (0..200).any(|_| {
        thread::sleep(Duration::from_millis(10));
        taking.is_finished()
    });
    assert!(ended, "the thread taking clients ends");
    assert!(!path.exists(), "removed");
    assert!(socket.close().is_none(), "nothing more to close");
}
