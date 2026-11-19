//! The pieces pairing and the channels share: keys from a secret, and
//! sealing with ChaCha20-Poly1305 under a nonce of 8 bytes.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use hkdf::Hkdf;
use sha2::Sha512;

use crate::error::{Error, Result};

/// A 32-byte key from `secret`: HKDF with SHA-512.
pub fn derive(secret: &[u8], salt: &str, info: &str) -> [u8; 32] {
    let mut key = [0; 32];
    Hkdf::<Sha512>::new(Some(salt.as_bytes()), secret)
        .expand(info.as_bytes(), &mut key)
        .expect("32 bytes is a length HKDF gives");
    key
}

/// The 12-byte nonce: four zeros, then the 8 bytes.
fn nonce(eight: [u8; 8]) -> [u8; 12] {
    let mut nonce = [0; 12];
    nonce[4..].copy_from_slice(&eight);
    nonce
}

/// A message's nonce, such as `PS-Msg05`.
pub fn message_nonce(name: &str) -> [u8; 8] {
    name.as_bytes().try_into().expect("an 8-byte message name")
}

/// The nonce of the `n`th frame.
pub fn counter_nonce(n: u64) -> [u8; 8] {
    n.to_le_bytes()
}

/// `plain`, sealed: the ciphertext, then the 16-byte tag.
pub fn seal(key: &[u8; 32], nonce8: [u8; 8], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    ChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(
            Nonce::from_slice(&nonce(nonce8)),
            Payload { msg: plain, aad },
        )
        .expect("sealing doesn't fail")
}

pub fn open(key: &[u8; 32], nonce8: [u8; 8], aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>> {
    ChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(
            Nonce::from_slice(&nonce(nonce8)),
            Payload { msg: sealed, aad },
        )
        .map_err(|_| Error::protocol("a message that doesn't decrypt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_what_it_sealed_and_nothing_tampered_with() {
        let key = [5; 32];
        let sealed = seal(&key, message_nonce("PS-Msg05"), b"aad", b"hello");
        assert_eq!(sealed.len(), 5 + 16);
        assert_eq!(
            open(&key, message_nonce("PS-Msg05"), b"aad", &sealed).unwrap(),
            b"hello"
        );
        assert!(open(&key, message_nonce("PS-Msg06"), b"aad", &sealed).is_err());
        assert!(open(&key, message_nonce("PS-Msg05"), b"other", &sealed).is_err());
    }

    #[test]
    fn a_frames_nonce_is_its_number_little_endian() {
        assert_eq!(counter_nonce(1), [1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(nonce(counter_nonce(258))[4..6], [2, 1]);
    }
}
