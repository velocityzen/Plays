//! SRP-6a (RFC 5054), as pairing has it: the 3072-bit group with SHA-512.
//! Both sides are here, the receiver's for the fake one.

use num_bigint::BigUint;
use sha2::Digest;

use crate::error::{Error, Result};

/// The 3072-bit group of RFC 5054, whose generator is 5.
const N_3072: &str = "\
    FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD129024E088A67CC74\
    020BBEA63B139B22514A08798E3404DDEF9519B3CD3A431B302B0A6DF25F1437\
    4FE1356D6D51C245E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED\
    EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3DC2007CB8A163BF05\
    98DA48361C55D39A69163FA8FD24CF5F83655D23DCA3AD961C62F356208552BB\
    9ED529077096966D670C354E4ABC9804F1746C08CA18217C32905E462E36CE3B\
    E39E772C180E86039B2783A2EC07A28FB5C55DF06F4C52C9DE2BCBF695581718\
    3995497CEA956AE515D2261898FA051015728E5A8AAAC42DAD33170D04507A33\
    A85521ABDF1CBA64ECFB850458DBEF0A8AEA71575D060C7DB3970F85A6E1E4C7\
    ABF5AE8CDB0933D71E8C94E04A25619DCEE3D2261AD2EE6BF12FFA06D98A0864\
    D87602733EC86A64521F2B18177B200CBBE117577A615D6C770988C0BAD946E2\
    08E24FA074E5AB3143DB5BFCE0FD108E4B82D120A93AD2CAFFFFFFFFFFFFFFFF";

#[derive(Clone)]
pub struct Group {
    n: BigUint,
    g: BigUint,
}

impl Group {
    /// The group pairing uses.
    pub fn pairing() -> Self {
        Self::new(N_3072, 5)
    }

    pub fn new(n: &str, g: u32) -> Self {
        let digits: String = n.chars().filter(|c| !c.is_whitespace()).collect();
        Self {
            n: BigUint::parse_bytes(digits.as_bytes(), 16).expect("a hex prime"),
            g: BigUint::from(g),
        }
    }

    /// Bytes a value of the group takes.
    fn size(&self) -> usize {
        self.n.bits().div_ceil(8) as usize
    }

    /// `value` as big-endian bytes, as many as the prime's.
    fn pad(&self, value: &BigUint) -> Vec<u8> {
        let bytes = value.to_bytes_be();
        let mut padded = vec![0; self.size().saturating_sub(bytes.len())];
        padded.extend(bytes);
        padded
    }
}

fn hash<D: Digest>(parts: &[&[u8]]) -> Vec<u8> {
    let mut digest = D::new();
    for part in parts {
        digest.update(part);
    }
    digest.finalize().to_vec()
}

fn number(bytes: &[u8]) -> BigUint {
    BigUint::from_bytes_be(bytes)
}

/// The multiplier: k = H(N | PAD(g)).
fn multiplier<D: Digest>(group: &Group) -> BigUint {
    number(&hash::<D>(&[&group.n.to_bytes_be(), &group.pad(&group.g)]))
}

/// The private key: x = H(s | H(I | ":" | P)).
fn private_key<D: Digest>(salt: &[u8], username: &[u8], password: &[u8]) -> BigUint {
    let inner = hash::<D>(&[username, b":", password]);
    number(&hash::<D>(&[salt, &inner]))
}

/// The scrambler: u = H(PAD(A) | PAD(B)).
fn scrambler<D: Digest>(group: &Group, a: &BigUint, b: &BigUint) -> BigUint {
    number(&hash::<D>(&[&group.pad(a), &group.pad(b)]))
}

/// The proof the client sends: H(H(N) xor H(g) | H(I) | s | A | B | K).
fn client_proof<D: Digest>(
    group: &Group,
    username: &[u8],
    salt: &[u8],
    a: &BigUint,
    b: &BigUint,
    key: &[u8],
) -> Vec<u8> {
    let n = hash::<D>(&[&group.n.to_bytes_be()]);
    let g = hash::<D>(&[&group.g.to_bytes_be()]);
    let ng: Vec<u8> = n.iter().zip(&g).map(|(n, g)| n ^ g).collect();
    hash::<D>(&[
        &ng,
        &hash::<D>(&[username]),
        salt,
        &group.pad(a),
        &group.pad(b),
        key,
    ])
}

/// What both sides end with: the session key, and the proofs of it.
pub struct Session {
    /// K = H(PAD(S)).
    pub key: Vec<u8>,
    /// The client's proof, M1.
    pub proof: Vec<u8>,
    /// The server's proof, M2 = H(A | M1 | K).
    pub server_proof: Vec<u8>,
}

fn session<D: Digest>(
    group: &Group,
    username: &[u8],
    salt: &[u8],
    a: &BigUint,
    b: &BigUint,
    premaster: &BigUint,
) -> Session {
    let key = hash::<D>(&[&group.pad(premaster)]);
    let proof = client_proof::<D>(group, username, salt, a, b, &key);
    let server_proof = hash::<D>(&[&group.pad(a), &proof, &key]);
    Session {
        key,
        proof,
        server_proof,
    }
}

/// The side that knows the password: the sender, pairing.
pub struct Client {
    group: Group,
    a: BigUint,
    public: BigUint,
}

impl Client {
    /// With the private value `a`, random bytes.
    pub fn new(group: Group, a: &[u8]) -> Self {
        let a = number(a);
        let public = group.g.modpow(&a, &group.n);
        Self { group, a, public }
    }

    /// A, to send.
    pub fn public(&self) -> Vec<u8> {
        self.group.pad(&self.public)
    }

    /// The session, from the server's salt and B.
    pub fn session<D: Digest>(
        &self,
        username: &[u8],
        password: &[u8],
        salt: &[u8],
        server_public: &[u8],
    ) -> Result<Session> {
        let group = &self.group;
        let b = number(server_public);
        if (&b % &group.n) == BigUint::ZERO {
            return Err(Error::pairing("the receiver's public key is no good"));
        }
        let u = scrambler::<D>(group, &self.public, &b);
        if u == BigUint::ZERO {
            return Err(Error::pairing("the receiver's public key is no good"));
        }
        let premaster = self.premaster::<D>(username, password, salt, &b, &u);
        Ok(session::<D>(
            group,
            username,
            salt,
            &self.public,
            &b,
            &premaster,
        ))
    }

    /// S = (B - k * g^x) ^ (a + u * x).
    fn premaster<D: Digest>(
        &self,
        username: &[u8],
        password: &[u8],
        salt: &[u8],
        b: &BigUint,
        u: &BigUint,
    ) -> BigUint {
        let group = &self.group;
        let x = private_key::<D>(salt, username, password);
        let k = multiplier::<D>(group);
        let kv = (k * group.g.modpow(&x, &group.n)) % &group.n;
        let base = (b + &group.n - kv) % &group.n;
        base.modpow(&(&self.a + u * x), &group.n)
    }
}

/// The side that keeps a verifier: the receiver.
#[cfg(any(test, feature = "fake"))]
pub struct Server {
    group: Group,
    b: BigUint,
    public: BigUint,
    verifier: BigUint,
    salt: Vec<u8>,
}

#[cfg(any(test, feature = "fake"))]
impl Server {
    /// For `password`, with `salt`, and the private value `b`.
    pub fn new<D: Digest>(
        group: Group,
        username: &[u8],
        password: &[u8],
        salt: &[u8],
        b: &[u8],
    ) -> Self {
        let x = private_key::<D>(salt, username, password);
        let verifier = group.g.modpow(&x, &group.n);
        let b = number(b);
        let k = multiplier::<D>(&group);
        let public = (k * &verifier + group.g.modpow(&b, &group.n)) % &group.n;
        Self {
            group,
            b,
            public,
            verifier,
            salt: salt.to_vec(),
        }
    }

    /// B, to send.
    pub fn public(&self) -> Vec<u8> {
        self.group.pad(&self.public)
    }

    /// The session, from the client's A; the client's proof is to be
    /// checked against it.
    pub fn session<D: Digest>(&self, username: &[u8], client_public: &[u8]) -> Result<Session> {
        let group = &self.group;
        let a = number(client_public);
        if (&a % &group.n) == BigUint::ZERO {
            return Err(Error::pairing("the sender's public key is no good"));
        }
        let u = scrambler::<D>(group, &a, &self.public);
        // S = (A * v^u) ^ b.
        let premaster =
            ((&a * self.verifier.modpow(&u, &group.n)) % &group.n).modpow(&self.b, &group.n);
        Ok(session::<D>(
            group,
            username,
            &self.salt,
            &a,
            &self.public,
            &premaster,
        ))
    }
}

#[cfg(test)]
#[path = "tests/srp.rs"]
mod tests;
