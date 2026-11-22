use super::*;

#[test]
fn reads_messages_one_after_another_with_their_bodies() {
    let first = Message {
        start: "RTSP/1.0 200 OK".into(),
        headers: vec![("CSeq".into(), "1".into())],
        body: b"hello".to_vec(),
    };
    let second = Message {
        start: "POST /feedback RTSP/1.0".into(),
        headers: vec![("CSeq".into(), "2".into())],
        body: Vec::new(),
    };
    let mut wire = first.encode();
    wire.extend(second.encode());
    let mut stream = io::Cursor::new(wire);
    let mut reader = Reader::default();
    let read = reader.read(&mut stream).unwrap().unwrap();
    assert_eq!(read.body, b"hello");
    assert_eq!(read.header("content-length"), Some("5"));
    assert_eq!(read.words(), ("RTSP/1.0", "200", "OK"));
    let read = reader.read(&mut stream).unwrap().unwrap();
    assert_eq!(read, second);
    assert_eq!(reader.read(&mut stream).unwrap(), None);
}

#[test]
fn a_message_cut_short_is_an_error() {
    let mut stream = io::Cursor::new(b"RTSP/1.0 200 OK\r\nContent-Length: 9\r\n\r\nshort".to_vec());
    assert!(Reader::default().read(&mut stream).is_err());
}
