//! The pieces pairing and the channels share: keys from a secret, and
//! sealing with ChaCha20-Poly1305 under a nonce of 8 bytes. And what
//! AirPlay 1's encrypted sound takes: AES-128 in CBC, and base64.

use aes::Aes128;
use aes::cipher::BlockEncrypt;
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

/// AES's block, in bytes.
pub const BLOCK: usize = 16;

/// `data`'s whole blocks encrypted in place, AES-128 in CBC from `iv`; the
/// bytes after the last whole block stay as they are, as AirPlay 1 does.
pub fn cbc_encrypt(key: &[u8; BLOCK], iv: &[u8; BLOCK], data: &mut [u8]) {
    let aes = Aes128::new(key.into());
    let mut previous = *iv;
    for block in data.as_chunks_mut::<BLOCK>().0 {
        for (byte, previous) in block.iter_mut().zip(previous) {
            *byte ^= previous;
        }
        aes.encrypt_block(block.into());
        previous = *block;
    }
}

/// What [`cbc_encrypt`] encrypted, decrypted in place.
#[cfg(any(test, feature = "fake"))]
pub fn cbc_decrypt(key: &[u8; BLOCK], iv: &[u8; BLOCK], data: &mut [u8]) {
    use aes::cipher::BlockDecrypt;
    let aes = Aes128::new(key.into());
    let mut previous = *iv;
    for block in data.as_chunks_mut::<BLOCK>().0 {
        let cipher = *block;
        aes.decrypt_block(block.into());
        for (byte, previous) in block.iter_mut().zip(previous) {
            *byte ^= previous;
        }
        previous = cipher;
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` in base64 with no padding, as AirPlay 1's SDP has its keys.
pub fn base64(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let number = chunk.iter().enumerate().fold(0u32, |number, (i, &byte)| {
            number | u32::from(byte) << (16 - 8 * i)
        });
        for i in 0..=chunk.len() {
            text.push(BASE64[(number >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    text
}

/// What `text` holds in base64, padded or not.
#[cfg(any(test, feature = "fake"))]
pub fn unbase64(text: &str) -> Option<Vec<u8>> {
    let digits = text
        .trim_end_matches('=')
        .bytes()
        .map(|digit| BASE64.iter().position(|&each| each == digit))
        .collect::<Option<Vec<_>>>()?;
    if digits.len() % 4 == 1 {
        return None;
    }
    let mut bytes = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let number = chunk.iter().enumerate().fold(0u32, |number, (i, &digit)| {
            number | (digit as u32) << (18 - 6 * i)
        });
        for i in 0..chunk.len() - 1 {
            bytes.push((number >> (16 - 8 * i)) as u8);
        }
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn encrypts_as_aes_128_in_cbc() {
        // NIST SP 800-38A, F.2.1: the first two blocks.
        let key = hex("2b7e151628aed2a6abf7158809cf4f3c").try_into().unwrap();
        let iv = hex("000102030405060708090a0b0c0d0e0f").try_into().unwrap();
        let plain = hex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51");
        let mut data = plain.clone();
        data.extend_from_slice(b"rest");
        cbc_encrypt(&key, &iv, &mut data);
        assert_eq!(
            data[..32],
            hex("7649abac8119b246cee98e9b12e9197d5086cb9b507219ee95db113a917678b2")
        );
        assert_eq!(&data[32..], b"rest", "what isn't a whole block stays");
        cbc_decrypt(&key, &iv, &mut data);
        assert_eq!(data[..32], plain);
        assert_eq!(&data[32..], b"rest");
    }

    #[test]
    fn writes_and_reads_base64() {
        for (bytes, text) in [
            (&b""[..], ""),
            (b"f", "Zg"),
            (b"fo", "Zm8"),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg"),
            (b"\xfb\xff\x00", "+/8A"),
        ] {
            assert_eq!(base64(bytes), text);
            assert_eq!(unbase64(text).as_deref(), Some(bytes));
        }
        assert_eq!(unbase64("Zm8=").as_deref(), Some(&b"fo"[..]));
        assert_eq!(unbase64("Z"), None);
        assert_eq!(unbase64("Zm!v"), None);
    }

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
