//! What goes over the channel: Chromium's `CastMessage` (from
//! `cast_channel.proto`), each after its length in four bytes, big-endian.
//! Every message here carries JSON.

use prost::Message as _;
use serde_json::Value;

/// The longest message a device sends or takes.
const MAX: usize = 64 * 1024;

pub const CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
pub const HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
pub const RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
pub const MEDIA: &str = "urn:x-cast:com.google.cast.media";

/// The device itself, as opposed to an app on it.
pub const PLATFORM: &str = "receiver-0";

#[derive(Clone, PartialEq, prost::Message)]
pub struct CastMessage {
    /// Always 0, CASTV2_1_0.
    #[prost(int32, required, tag = "1")]
    pub protocol_version: i32,
    #[prost(string, required, tag = "2")]
    pub source_id: String,
    #[prost(string, required, tag = "3")]
    pub destination_id: String,
    #[prost(string, required, tag = "4")]
    pub namespace: String,
    /// 0 for text, 1 for binary.
    #[prost(int32, required, tag = "5")]
    pub payload_type: i32,
    #[prost(string, optional, tag = "6")]
    pub payload_utf8: Option<String>,
    #[prost(bytes = "vec", optional, tag = "7")]
    pub payload_binary: Option<Vec<u8>>,
}

impl CastMessage {
    pub fn json(source: &str, destination: &str, namespace: &str, payload: &Value) -> Self {
        Self {
            protocol_version: 0,
            source_id: source.to_owned(),
            destination_id: destination.to_owned(),
            namespace: namespace.to_owned(),
            payload_type: 0,
            payload_utf8: Some(payload.to_string()),
            payload_binary: None,
        }
    }

    /// The JSON it carries, or `Null`.
    pub fn payload(&self) -> Value {
        self.payload_utf8
            .as_deref()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or(Value::Null)
    }

    /// Its length, then itself.
    pub fn frame(&self) -> Vec<u8> {
        let body = self.encode_to_vec();
        let mut frame = Vec::with_capacity(4 + body.len());
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend(body);
        frame
    }
}

/// The type a payload says it is.
pub fn kind(payload: &Value) -> &str {
    payload["type"].as_str().unwrap_or_default()
}

/// The first whole message in `buffer`, taken out of it.
pub fn take(buffer: &mut Vec<u8>) -> std::io::Result<Option<CastMessage>> {
    let Some(length) = buffer.first_chunk::<4>() else {
        return Ok(None);
    };
    let length = u32::from_be_bytes(*length) as usize;
    if length > MAX {
        return Err(std::io::Error::other(format!(
            "a message of {length} bytes"
        )));
    }
    if buffer.len() < 4 + length {
        return Ok(None);
    }
    let message = CastMessage::decode(&buffer[4..4 + length]).map_err(std::io::Error::other);
    buffer.drain(..4 + length);
    message.map(Some)
}

#[cfg(test)]
mod tests {
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
}
