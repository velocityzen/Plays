//! RSA as AirPlay 1 wraps the key its sound is encrypted with: OAEP, with
//! SHA-1 for the hash and the mask, under the public key whose private
//! half every AirPort Express holds, as does every speaker made to take
//! what one takes.

use num_bigint::BigUint;
use sha1::{Digest, Sha1};

/// The AirPort Express's public modulus.
const AIRPORT_MODULUS: &str = concat!(
    "e7d744f2a2e2788b6c1f55a08eb70544a8fa7945aa8be6c62ce5f51cbdd4dc68",
    "42fe3d1083dd2edec1bfd4252dc02e6f398bdf0e6148ea84855e2e442da6d626",
    "64f674a1f304929ade4f6893ef2df6e711a8c77a0d91c9d980822e50d12922af",
    "ea40ea9f0e14c0f76938c5f3882fc0323dd9fe55155f51bb5921c201629fd733",
    "52d5e2efaabf9ba048d7b813a2b6767f6c3ccf1eb4ce673d037b0d2ea30c5fff",
    "eb06f8d08adde409571a9c689fef10728855dd8cfb9a8bef5c8943ef3b5faa15",
    "dde698beddf3599603eb3e6f61372bb628f6559f599a78bf500687aa7f4976c0",
    "562d412956f8989e18a6355bd81597825e0fc875343ec782117625cdbf98447b",
);
const EXPONENT: u32 = 65_537;
/// SHA-1's length.
const HASH: usize = 20;

#[derive(Clone, PartialEq, Debug)]
pub struct PublicKey {
    modulus: BigUint,
}

impl PublicKey {
    pub fn airport() -> Self {
        Self::from_hex(AIRPORT_MODULUS)
    }

    fn from_hex(hex: &str) -> Self {
        Self {
            modulus: BigUint::parse_bytes(hex.as_bytes(), 16).expect("a modulus in hex"),
        }
    }

    /// How long its numbers are, in bytes.
    fn size(&self) -> usize {
        (self.modulus.bits() as usize).div_ceil(8)
    }

    /// `message`, encrypted: it is at most the key's size less 42 bytes.
    pub fn encrypt(&self, message: &[u8]) -> Vec<u8> {
        let size = self.size();
        assert!(
            message.len() + 2 * HASH + 2 <= size,
            "a message too long for the key"
        );
        // The label's hash (of none), zeros, a one, then the message.
        let mut block = vec![0; size - HASH - 1];
        block[..HASH].copy_from_slice(&Sha1::digest(b""));
        let one = block.len() - message.len() - 1;
        block[one] = 1;
        block[one + 1..].copy_from_slice(message);
        let mut seed: [u8; HASH] = rand::random();
        xor(&mut block, &mask(&seed, size - HASH - 1));
        xor(&mut seed, &mask(&block, HASH));
        let mut encoded = Vec::with_capacity(size);
        encoded.push(0);
        encoded.extend_from_slice(&seed);
        encoded.extend_from_slice(&block);
        let cipher =
            BigUint::from_bytes_be(&encoded).modpow(&BigUint::from(EXPONENT), &self.modulus);
        padded(&cipher, size)
    }
}

/// MGF1 with SHA-1: `length` bytes from `seed`.
fn mask(seed: &[u8], length: usize) -> Vec<u8> {
    let mut mask = Vec::with_capacity(length + HASH);
    let mut counter = 0u32;
    while mask.len() < length {
        let mut hash = Sha1::new();
        hash.update(seed);
        hash.update(counter.to_be_bytes());
        mask.extend_from_slice(&hash.finalize());
        counter += 1;
    }
    mask.truncate(length);
    mask
}

fn xor(bytes: &mut [u8], mask: &[u8]) {
    for (byte, mask) in bytes.iter_mut().zip(mask) {
        *byte ^= mask;
    }
}

/// `number`'s bytes, big-endian, `size` of them.
fn padded(number: &BigUint, size: usize) -> Vec<u8> {
    let bytes = number.to_bytes_be();
    let mut padded = vec![0; size.saturating_sub(bytes.len())];
    padded.extend(bytes);
    padded
}

/// The key fake speakers hold, made for them: its private half is no
/// secret, and has nothing to do with the AirPort's.
#[cfg(any(test, feature = "fake"))]
const FAKE_MODULUS: &str = concat!(
    "ba873b838c9435f61d6dfc4ab45bb7f086adf96068b788e0b55a76fe7ea6d55d",
    "df50dd0a0dace637e7f9935fa50b00532191bedc9aba8eaf27afa8ef437a3d1d",
    "14cfd569b0ac5409aee495ce06206f61d59ece653d1ec018cb22b05a53d85fa7",
    "642e661b77fcaf550d0c55a7039811b64cbdaf95d0ee2411c9ef24d40b9fa4f2",
    "08f5128d5d2877fc25a0d1fa2665aa3a4ba332d5cd8550189cebade1480fd686",
    "9249e7fef1f42f96a013c1c1239e3d3b5f627ced2663e533805ab46ebabb5673",
    "7b37781ea6d77cb0dbad7e141b93eea4bf920344b88f6499c44ffdbad6de48ae",
    "b3bc6788a5ea76e0ec1954973b5f1bed20e09de9c342f6efa49d5848ab2760d7",
);
#[cfg(any(test, feature = "fake"))]
const FAKE_PRIVATE_EXPONENT: &str = concat!(
    "0dc90bcd7398ca8a276fa6fb306c2ce27259ae3a01533b22fc5434c227431636",
    "cb57d316389f841e329f3fd48c428d621773898b0d4667602b8b306d8a6ba0d3",
    "a2e39f4ea1d782987a9bfb3450409a6abd1a70ff8b6e57dd1d3a31956367f4dd",
    "ceaa1d32bf81e5c55fe2fefb8a68f53a275ec778c789aa11c47e4f96fa4af0aa",
    "305073b03ab32a7a517db5a0744a2e1faa5771ded955cabcfde3b0ad906ea873",
    "57013b6644898b33ccff34f5e7d33f797ab8d2baf1ef8b2381acbb1f9a432ea6",
    "e6013ce8daf0343b6f1ea7b437cae12402bb638303eacd053eabb19f6703c6e8",
    "a628873d2bdf2b8b7dc3b0f4b5864d46df4758f72e0c25d907a7da7a02efb149",
);

/// A private key, for a fake speaker to unwrap the sound's key with.
#[cfg(any(test, feature = "fake"))]
pub struct PrivateKey {
    public: PublicKey,
    exponent: BigUint,
}

#[cfg(any(test, feature = "fake"))]
impl PrivateKey {
    pub fn fake() -> Self {
        Self {
            public: PublicKey::from_hex(FAKE_MODULUS),
            exponent: BigUint::parse_bytes(FAKE_PRIVATE_EXPONENT.as_bytes(), 16)
                .expect("an exponent in hex"),
        }
    }

    pub fn public(&self) -> &PublicKey {
        &self.public
    }

    /// What [`PublicKey::encrypt`] encrypted, if it was this key's.
    pub fn decrypt(&self, cipher: &[u8]) -> Option<Vec<u8>> {
        let size = self.public.size();
        if cipher.len() != size {
            return None;
        }
        let number = BigUint::from_bytes_be(cipher).modpow(&self.exponent, &self.public.modulus);
        let encoded = padded(&number, size);
        if encoded[0] != 0 {
            return None;
        }
        let (seed, block) = encoded[1..].split_at(HASH);
        let mut seed = seed.to_vec();
        xor(&mut seed, &mask(block, HASH));
        let mut block = block.to_vec();
        let length = block.len();
        xor(&mut block, &mask(&seed, length));
        if block[..HASH] != Sha1::digest(b"")[..] {
            return None;
        }
        let rest = &block[HASH..];
        let one = rest.iter().position(|&byte| byte != 0)?;
        (rest[one] == 1).then(|| rest[one + 1..].to_vec())
    }
}

#[cfg(test)]
#[path = "tests/rsa.rs"]
mod tests;
