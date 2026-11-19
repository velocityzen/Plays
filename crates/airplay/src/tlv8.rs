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
mod tests {
    use super::*;

    #[test]
    fn a_long_value_goes_in_pieces_and_comes_back_whole() {
        let key: Vec<u8> = (0..384).map(|i| i as u8).collect();
        let tlv = Tlv::new()
            .byte(Tag::State, 3)
            .with(Tag::PublicKey, &key)
            .with(Tag::Proof, [7; 64]);
        let bytes = tlv.encode();
        assert_eq!(&bytes[..3], [0x06, 1, 3]);
        assert_eq!(&bytes[3..5], [0x03, 255]);
        assert_eq!(&bytes[260..262], [0x03, 129]);
        assert_eq!(bytes.len(), 3 + 2 + 255 + 2 + 129 + 2 + 64);
        let back = Tlv::decode(&bytes).unwrap();
        assert_eq!(back, tlv);
        assert_eq!(back.get(Tag::PublicKey), Some(key.as_slice()));
        assert_eq!(back.state(), Some(3));
    }

    #[test]
    fn a_value_of_exactly_255_bytes_isnt_joined_to_the_next_item_of_another_kind() {
        let tlv = Tlv::new()
            .with(Tag::Salt, [1; 255])
            .with(Tag::PublicKey, [2; 3]);
        assert_eq!(Tlv::decode(&tlv.encode()).unwrap(), tlv);
    }

    #[test]
    fn a_message_cut_short_is_an_error() {
        assert!(Tlv::decode(&[0x06, 2, 1]).is_err());
        assert!(Tlv::decode(&[0x06]).is_err());
    }
}
