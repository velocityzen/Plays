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
#[path = "tests/proto_properties.rs"]
mod properties;
#[cfg(test)]
#[path = "tests/proto.rs"]
mod tests;
