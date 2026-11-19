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
mod tests {
    use super::*;
    use sha1::Sha1;
    use sha2::Sha512;

    /// RFC 5054's 1024-bit group, which its test vectors use.
    const N_1024: &str = "\
        EEAF0AB9ADB38DD69C33F80AFA8FC5E86072618775FF3C0B9EA2314C9C256576\
        D674DF7496EA81D3383B4813D692C6E0E0D5D8E250B98BE48E495C1D6089DAD1\
        5DC7D7B46154D6B6CE8EF4AD69B15D4982559B297BCF1885C529F566660E57EC\
        68EDBC3C05726CC02FD4CBF4976EAA9AFD5138FE8376435B9FC61D2FC0EB06E3";

    fn hex(digits: &str) -> BigUint {
        let digits: String = digits.chars().filter(|c| !c.is_whitespace()).collect();
        BigUint::parse_bytes(digits.as_bytes(), 16).unwrap()
    }

    fn bytes(digits: &str) -> Vec<u8> {
        hex(digits).to_bytes_be()
    }

    // RFC 5054, appendix B.
    const SALT: &str = "BEB25379 D1A8581E B5A72767 3A2441EE";
    const A_PRIVATE: &str = "60975527 035CF2AD 1989806F 0407210B C81EDC04 E2762A56 AFD529DD
        DA2D4393";
    const B_PRIVATE: &str = "E487CB59 D31AC550 471E81F0 0F6928E0 1DDA08E9 74A004F4 9E61F5D1
        05284D20";
    const VERIFIER: &str = "7E273DE8 696FFC4F 4E337D05 B4B375BE B0DDE156 9E8FA00A 9886D812
        9BADA1F1 822223CA 1A605B53 0E379BA4 729FDC59 F105B478 7E5186F5
        C671085A 1447B52A 48CF1970 B4FB6F84 00BBF4CE BFBB1681 52E08AB5
        EA53D15C 1AFF87B2 B9DA6E04 E058AD51 CC72BFC9 033B564E 26480D78
        E955A5E2 9E7AB245 DB2BE315 E2099AFB";
    const A_PUBLIC: &str = "61D5E490 F6F1B795 47B0704C 436F523D D0E560F0 C64115BB 72557EC4
        4352E890 3211C046 92272D8B 2D1A5358 A2CF1B6E 0BFCF99F 921530EC
        8E393561 79EAE45E 42BA92AE ACED8251 71E1E8B9 AF6D9C03 E1327F44
        BE087EF0 6530E69F 66615261 EEF54073 CA11CF58 58F0EDFD FE15EFEA
        B349EF5D 76988A36 72FAC47B 0769447B";
    const B_PUBLIC: &str = "BD0C6151 2C692C0C B6D041FA 01BB152D 4916A1E7 7AF46AE1 05393011
        BAF38964 DC46A067 0DD125B9 5A981652 236F99D9 B681CBF8 7837EC99
        6C6DA044 53728610 D0C6DDB5 8B318885 D7D82C7F 8DEB75CE 7BD4FBAA
        37089E6F 9C6059F3 88838E7A 00030B33 1EB76840 910440B1 B27AAEAE
        EB4012B7 D7665238 A8E3FB00 4B117B58";
    const PREMASTER: &str = "B0DC82BA BCF30674 AE450C02 87745E79 90A3381F 63B387AA F271A10D
        233861E3 59B48220 F7C4693C 9AE12B0A 6F67809F 0876E2D0 13800D6C
        41BB59B6 D5979B5C 00A172B4 A2A5903A 0BDCAF8A 709585EB 2AFAFA8F
        3499B200 210DCC1F 10EB3394 3CD67FC8 8A2F39A4 BE5BEC4E C0A3212D
        C346D7E4 74B29EDE 8A469FFE CA686E5A";

    fn group() -> Group {
        Group::new(N_1024, 2)
    }

    #[test]
    fn matches_the_rfc_test_vectors_step_by_step() {
        let group = group();
        let salt = bytes(SALT);
        assert_eq!(
            multiplier::<Sha1>(&group),
            hex("7556AA04 5AEF2CDD 07ABAF0F 665C3E81 8913186F")
        );
        let x = private_key::<Sha1>(&salt, b"alice", b"password123");
        assert_eq!(x, hex("94B7555A ABE9127C C58CCF49 93DB6CF8 4D16C124"));
        assert_eq!(group.g.modpow(&x, &group.n), hex(VERIFIER));

        let client = Client::new(group.clone(), &bytes(A_PRIVATE));
        assert_eq!(client.public(), bytes(A_PUBLIC));
        let server = Server::new::<Sha1>(
            group.clone(),
            b"alice",
            b"password123",
            &salt,
            &bytes(B_PRIVATE),
        );
        assert_eq!(server.public(), bytes(B_PUBLIC));

        let u = scrambler::<Sha1>(&group, &hex(A_PUBLIC), &hex(B_PUBLIC));
        assert_eq!(u, hex("CE38B959 3487DA98 554ED47D 70A7AE5F 462EF019"));
        let premaster =
            client.premaster::<Sha1>(b"alice", b"password123", &salt, &hex(B_PUBLIC), &u);
        assert_eq!(premaster, hex(PREMASTER));
    }

    #[test]
    fn both_sides_agree_on_the_key_and_the_proofs_with_the_right_password() {
        let group = Group::pairing();
        let salt = [9; 16];
        let server = Server::new::<Sha512>(group.clone(), b"Pair-Setup", b"3939", &salt, &[3; 32]);
        let client = Client::new(group, &[4; 32]);
        assert_eq!(client.public().len(), 384);
        let ours = client
            .session::<Sha512>(b"Pair-Setup", b"3939", &salt, &server.public())
            .unwrap();
        let theirs = server
            .session::<Sha512>(b"Pair-Setup", &client.public())
            .unwrap();
        assert_eq!(ours.key, theirs.key);
        assert_eq!(ours.key.len(), 64);
        assert_eq!(ours.proof, theirs.proof);
        assert_eq!(ours.server_proof, theirs.server_proof);

        let wrong = Client::new(Group::pairing(), &[4; 32])
            .session::<Sha512>(b"Pair-Setup", b"1234", &salt, &server.public())
            .unwrap();
        assert_ne!(wrong.proof, theirs.proof);
    }

    #[test]
    fn a_public_key_of_zero_is_refused() {
        let client = Client::new(Group::pairing(), &[4; 32]);
        assert!(
            client
                .session::<Sha512>(b"Pair-Setup", b"3939", &[1; 16], &[0; 384])
                .is_err()
        );
    }
}
