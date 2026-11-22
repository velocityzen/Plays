use serde_json::json;

use super::*;

#[test]
fn a_message_is_its_length_then_the_protobuf() {
    let message = CastMessage::json(
        "sender-0",
        PLATFORM,
        CONNECTION,
        &json!({"type": "CONNECT"}),
    );
    let frame = message.frame();
    let mut expected = vec![0x08, 0x00, 0x12, 8];
    expected.extend(b"sender-0");
    expected.extend([0x1A, 10]);
    expected.extend(b"receiver-0");
    expected.extend([0x22, CONNECTION.len() as u8]);
    expected.extend(CONNECTION.as_bytes());
    expected.extend([0x28, 0x00, 0x32, 18]);
    expected.extend(br#"{"type":"CONNECT"}"#);
    assert_eq!(frame[..4], (expected.len() as u32).to_be_bytes());
    assert_eq!(frame[4..], expected);
}

#[test]
fn messages_come_out_whole_one_at_a_time() {
    let first = CastMessage::json("a", "b", HEARTBEAT, &json!({"type": "PING"}));
    let second = CastMessage::json("b", "a", HEARTBEAT, &json!({"type": "PONG"}));
    let mut buffer = first.frame();
    buffer.extend(second.frame());
    let cut = buffer.split_off(buffer.len() - 3);

    assert_eq!(take(&mut buffer).unwrap(), Some(first));
    assert_eq!(take(&mut buffer).unwrap(), None, "not all there yet");
    buffer.extend(cut);
    let second = take(&mut buffer).unwrap().unwrap();
    assert_eq!(kind(&second.payload()), "PONG");
    assert!(buffer.is_empty());
}

#[test]
fn a_message_too_long_is_an_error() {
    let mut buffer = (MAX as u32 + 1).to_be_bytes().to_vec();
    assert!(take(&mut buffer).is_err());
}
