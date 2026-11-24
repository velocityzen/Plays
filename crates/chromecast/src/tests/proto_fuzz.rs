use hegel::TestCase;
use hegel::generators as gs;

use super::*;

/// What a device might send: messages, a length with anything after it,
/// or any bytes, one after another.
fn wire(tc: &TestCase) -> Vec<u8> {
    let mut wire = Vec::new();
    for _ in 0..tc.draw(gs::integers::<usize>().max_value(6)) {
        match tc.draw(gs::integers::<u8>().max_value(2)) {
            0 => wire.extend(message(tc).frame()),
            1 => {
                let length = tc.draw(hegel::one_of!(
                    gs::sampled_from(vec![0, 1, 5, MAX as u32, MAX as u32 + 1, u32::MAX]),
                    gs::integers::<u32>()
                ));
                wire.extend(length.to_be_bytes());
                wire.extend(tc.draw(gs::binary().max_size(16)));
            }
            _ => wire.extend(tc.draw(gs::binary().max_size(8))),
        }
    }
    wire
}

/// A message with JSON, as devices send, or any text or bytes.
fn message(tc: &TestCase) -> CastMessage {
    let names = vec!["sender-0", PLATFORM, "web-5"];
    let namespaces = vec![CONNECTION, HEARTBEAT, RECEIVER, MEDIA];
    CastMessage {
        protocol_version: tc.draw(gs::integers::<i32>()),
        source_id: tc.draw(gs::sampled_from(names.clone())).to_owned(),
        destination_id: tc.draw(gs::sampled_from(names)).to_owned(),
        namespace: tc.draw(gs::sampled_from(namespaces)).to_owned(),
        payload_type: tc.draw(gs::integers::<i32>().min_value(0).max_value(1)),
        payload_utf8: tc.draw(gs::optional(hegel::one_of!(
            gs::sampled_from(vec![
                r#"{"type":"PING"}"#.to_owned(),
                r#"{"type":"MEDIA_STATUS","status":[]}"#.to_owned(),
            ]),
            gs::text().max_size(32)
        ))),
        payload_binary: tc.draw(gs::optional(gs::binary().max_size(16))),
    }
}

/// The messages taken from `wire` as it comes `step` bytes at a time, to
/// its end or the first that isn't one; what's held while waiting for
/// more is never more than a message.
fn taken(wire: &[u8], step: usize) -> Vec<Result<CastMessage, String>> {
    let mut buffer = Vec::new();
    let mut taken = Vec::new();
    for piece in wire.chunks(step) {
        buffer.extend_from_slice(piece);
        loop {
            match take(&mut buffer) {
                Ok(Some(message)) => taken.push(Ok(message)),
                Ok(None) => break,
                Err(err) => {
                    taken.push(Err(err.to_string()));
                    return taken;
                }
            }
        }
        assert!(buffer.len() < 4 + MAX, "{} bytes held", buffer.len());
    }
    taken
}

#[hegel::test]
fn any_bytes_give_the_same_messages_however_they_arrive(tc: TestCase) {
    let wire = wire(&tc);
    let step = tc.draw(gs::integers::<usize>().min_value(1).max_value(64));
    assert_eq!(taken(&wire, step), taken(&wire, wire.len().max(1)));
}
