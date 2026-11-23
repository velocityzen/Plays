use super::*;

#[test]
fn reads_byte_ranges() {
    assert_eq!(range(None, 100), Ok(None));
    assert_eq!(range(Some("bytes=0-"), 100), Ok(Some((0, 99))));
    assert_eq!(range(Some("bytes=10-19"), 100), Ok(Some((10, 19))));
    assert_eq!(range(Some("bytes=90-200"), 100), Ok(Some((90, 99))));
    assert_eq!(range(Some("bytes=-10"), 100), Ok(Some((90, 99))));
    assert_eq!(range(Some("bytes=-500"), 100), Ok(Some((0, 99))));
    assert_eq!(range(Some("bytes=0-1,5-6"), 100), Ok(None));
    assert_eq!(range(Some("bytes=100-"), 100), Err(()));
    assert_eq!(range(Some("bytes=20-10"), 100), Err(()));
    assert_eq!(range(Some("bytes=x-"), 100), Err(()));
    assert_eq!(range(Some("bytes=0-"), 0), Err(()));
    assert_eq!(range(Some("items=0-1"), 100), Ok(None));
}

/// A reader counting what's read from it.
struct Counted<R> {
    inner: R,
    read: usize,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read += n;
        Ok(n)
    }
}

/// What reading a request from `head` and then `filler` gives, and how
/// much was read.
pub(super) fn read_from(head: &str, filler: impl Read) -> (io::Result<bool>, usize) {
    let mut reader = BufReader::new(Counted {
        inner: head.as_bytes().chain(filler),
        read: 0,
    });
    let request = read_request(&mut reader).map(|request| request.is_some());
    (request, reader.get_ref().read)
}

#[test]
fn a_head_too_long_is_refused_before_it_is_all_read() {
    let endless = || io::repeat(b'a').take(8 << 20);
    for (head, filler) in [
        ("", Box::new(endless()) as Box<dyn Read>),
        ("GET / HTTP/1.1\r\nX-Long: ", Box::new(endless())),
        ("", Box::new(io::Cursor::new(b"\r\n".repeat(1 << 20)))),
    ] {
        let (request, read) = read_from(head, filler);
        assert_eq!(
            request.map_err(|err| err.kind()).err(),
            Some(io::ErrorKind::InvalidData),
            "after {head:?}"
        );
        assert!(read < 2 * MAX_HEAD, "{read} bytes read after {head:?}");
    }
    let (request, _) = read_from(
        "\r\nGET /a?b HTTP/1.1\r\nRange: bytes=0-\r\n\r\n",
        io::empty(),
    );
    assert!(request.unwrap(), "a head as long as it may be is read");
}

/// A server on this computer answering every request with 200.
fn server() -> std::net::SocketAddr {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let handler: Handler = Arc::new(|_: &Request| Response::status(200));
    thread::spawn(move || {
        serve(
            listener,
            handler,
            |_| true,
            Arc::new(AtomicBool::new(false)),
        )
    });
    address
}

/// Whether a request on `stream` is answered, rather than the
/// connection closed.
fn answered(mut stream: TcpStream) -> bool {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let asked = stream.write_all(b"GET / HTTP/1.1\r\nConnection: close\r\n\r\n");
    let mut response = Vec::new();
    let _ = stream.read_to_end(&mut response);
    asked.is_ok() && response.starts_with(b"HTTP/1.1 200")
}

#[test]
fn connections_past_the_most_served_at_once_are_closed() {
    let address = server();
    let open: Vec<_> = (0..MAX_CONNECTIONS)
        .map(|_| TcpStream::connect(address).unwrap())
        .collect();
    assert!(
        !answered(TcpStream::connect(address).unwrap()),
        "one too many is closed"
    );
    // Their threads see them closed, and give their places back.
    drop(open);
    let start = Instant::now();
    while !answered(TcpStream::connect(address).unwrap()) {
        assert!(start.elapsed() < IDLE * 5, "no place came free");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_head_sent_a_byte_at_a_time_takes_no_longer_than_a_whole_one_may() {
    let address = server();
    let stream = TcpStream::connect(address).unwrap();
    let mut writer = stream.try_clone().unwrap();
    // Each byte comes well within the time a read waits.
    thread::spawn(move || {
        let head = b"GET / HTTP/1.1\r\nX-Slow: ".iter();
        for byte in head.chain(std::iter::repeat(&b'a')).take(40) {
            if writer.write_all(&[*byte]).is_err() {
                break;
            }
            thread::sleep(IDLE / 4);
        }
    });
    let start = Instant::now();
    let mut reader = stream;
    reader.set_read_timeout(Some(IDLE * 5)).unwrap();
    let closed = match reader.read(&mut [0; 64]) {
        Ok(n) => n == 0,
        Err(err) => !matches!(
            err.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ),
    };
    assert!(closed, "still open after {:?}", start.elapsed());
    assert!(
        start.elapsed() < IDLE * 2,
        "closed after {:?}",
        start.elapsed()
    );
}
