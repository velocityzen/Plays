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
