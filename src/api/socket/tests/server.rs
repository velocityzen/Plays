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
    assert_eq!(waiting.try_iter().collect::<Vec<_>>(), ["one"]);
    assert_eq!(open(&mut read).unwrap(), 0, "cut off");
}
