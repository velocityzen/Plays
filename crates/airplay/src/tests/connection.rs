use std::net::{Ipv4Addr, TcpListener};
use std::time::Instant;

use super::*;

/// A receiver that reads the first request it's sent, then answers as
/// `answer` does.
fn receiver(answer: impl FnOnce(&mut TcpStream) + Send + 'static) -> SocketAddr {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        Reader::default().read(&mut stream).unwrap().unwrap();
        answer(&mut stream);
    });
    address
}

#[test]
fn an_answer_longer_than_any_may_be_is_an_error() {
    let address = receiver(|stream| {
        let _ = stream.write_all(
            b"RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Length: 18446744073709551615\r\n\r\n",
        );
    });
    let mut connection = Connection::open(&[address]).unwrap();
    assert!(connection.send(Request::rtsp("OPTIONS", "*")).is_err());
}

#[test]
fn an_answer_past_the_most_a_body_may_be_is_an_error_though_it_all_comes() {
    let address = receiver(|stream| {
        let length = 1024 * 1024 + 1;
        let head = format!("RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Length: {length}\r\n\r\n");
        let mut answer = head.into_bytes();
        answer.resize(answer.len() + length, b'x');
        let _ = stream.write_all(&answer);
    });
    let mut connection = Connection::open(&[address]).unwrap();
    assert!(connection.send(Request::rtsp("OPTIONS", "*")).is_err());
}

#[test]
fn an_answer_sent_a_byte_at_a_time_takes_no_longer_than_a_whole_one_may() {
    let address = receiver(|stream| {
        let _ = stream.write_all(b"RTSP/1.0 200 OK\r\n");
        // Each byte well within the wait, but never the whole head.
        for _ in 0..200 {
            std::thread::sleep(REPLY_TIMEOUT / 20);
            if stream.write_all(b"X").is_err() {
                return;
            }
        }
    });
    let mut connection = Connection::open(&[address]).unwrap();
    let asked = Instant::now();
    assert!(connection.send(Request::rtsp("OPTIONS", "*")).is_err());
    let took = asked.elapsed();
    assert!(took < REPLY_TIMEOUT * 2, "it took {took:?}");
}

#[test]
fn an_answer_to_another_request_is_refused_and_the_connection_let_go() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let receiver = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = Reader::default();
        reader.read(&mut stream).unwrap().unwrap();
        stream
            .write_all(b"RTSP/1.0 200 OK\r\nCSeq: 7\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        // Whether the sender closed the connection.
        reader.read(&mut stream).is_ok_and(|next| next.is_none())
    });
    let mut connection = Connection::open(&[address]).unwrap();
    assert!(matches!(
        connection.send(Request::rtsp("OPTIONS", "*")),
        Err(Error::Protocol(_))
    ));
    assert!(matches!(
        connection.send(Request::rtsp("OPTIONS", "*")),
        Err(Error::Io(err)) if err.kind() == std::io::ErrorKind::NotConnected
    ));
    assert!(receiver.join().unwrap(), "the connection is still open");
}

/// Encrypted, the frame the answer comes in is read as one: each byte
/// of it may not have all the time left afresh.
#[test]
fn an_encrypted_answer_sent_a_byte_at_a_time_takes_no_longer_either() {
    let keys = Keys::control(b"a secret both sides have");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut sealed = Sealed::new(stream, keys.swapped());
        Reader::default().read(&mut sealed).unwrap().unwrap();
        let mut frame = Sealed::new(Vec::new(), keys.swapped());
        frame
            .write_all(b"RTSP/1.0 200 OK\r\nCSeq: 1\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        let mut stream = sealed.get_ref();
        for byte in frame.get_ref() {
            std::thread::sleep(REPLY_TIMEOUT / 20);
            if stream.write_all(&[*byte]).is_err() {
                return;
            }
        }
    });
    let mut connection = Connection::open(&[address]).unwrap();
    connection.seal(keys);
    let asked = Instant::now();
    assert!(connection.send(Request::rtsp("OPTIONS", "*")).is_err());
    let took = asked.elapsed();
    assert!(took < REPLY_TIMEOUT * 2, "it took {took:?}");
}

#[test]
fn connects_at_the_first_address_that_takes_it() {
    let closed = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let open = listener.local_addr().unwrap();
    let connection = Connection::open(&[closed, open]).unwrap();
    assert_eq!(connection.remote, open);
    assert!(Connection::open(&[closed]).is_err());
    assert!(Connection::open(&[]).is_err());
}
