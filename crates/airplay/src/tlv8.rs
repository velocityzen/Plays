//! TLV8, the type-length-value encoding of pairing messages: a byte of
//! type, a byte of length, then the value. A value longer than 255 bytes
//! goes as pieces of the same type in a row.

use crate::error::{Error, Result};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Tag {
    Method = 0x00,
    Identifier = 0x01,
    Salt = 0x02,
    PublicKey = 0x03,
    Proof = 0x04,
    EncryptedData = 0x05,
    State = 0x06,
    Error = 0x07,
    Signature = 0x0A,
    Flags = 0x13,
}

/// A pairing message's items, in order.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Tlv {
    items: Vec<(u8, Vec<u8>)>,
}

impl Tlv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, tag: Tag, value: impl AsRef<[u8]>) -> Self {
        self.items.push((tag as u8, value.as_ref().to_vec()));
        self
    }

    pub fn byte(self, tag: Tag, value: u8) -> Self {
        self.with(tag, [value])
    }

    pub fn get(&self, tag: Tag) -> Option<&[u8]> {
        self.items
            .iter()
            .find(|(found, _)| *found == tag as u8)
            .map(|(_, value)| value.as_slice())
    }

    /// The value of `tag`, which the message must have.
    pub fn need(&self, tag: Tag) -> Result<&[u8]> {
        self.get(tag)
            .ok_or_else(|| Error::protocol(format!("a pairing message without {tag:?}")))
    }

    pub fn state(&self) -> Option<u8> {
        self.get(Tag::State)
            .and_then(|value| value.first().copied())
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (tag, value) in &self.items {
            if value.is_empty() {
                bytes.extend([*tag, 0]);
                continue;
            }
            for piece in value.chunks(255) {
                bytes.push(*tag);
                bytes.push(piece.len() as u8);
                bytes.extend_from_slice(piece);
            }
        }
        bytes
    }

    pub fn decode(mut bytes: &[u8]) -> Result<Self> {
        let mut tlv = Self::new();
        // Whether the item before was a full piece, so this one goes on it.
        let mut continues = false;
        while !bytes.is_empty() {
            let [tag, length, rest @ ..] = bytes else {
                return Err(Error::protocol("a pairing message cut short"));
            };
            let length = usize::from(*length);
            if rest.len() < length {
                return Err(Error::protocol("a pairing message cut short"));
            }
            let value = &rest[..length];
            match tlv.items.last_mut() {
                Some((last, so_far)) if continues && last == tag => so_far.extend_from_slice(value),
                _ => tlv.items.push((*tag, value.to_vec())),
            }
            continues = length == 255;
            bytes = &rest[length..];
        }
        Ok(tlv)
    }
}

#[cfg(test)]
#[path = "tests/tlv8_properties.rs"]
mod properties;
#[cfg(test)]
#[path = "tests/tlv8.rs"]
mod tests;
