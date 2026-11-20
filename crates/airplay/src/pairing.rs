//! Pairing, as HomeKit has it. Pair-setup with the PIN a receiver shows
//! leaves both sides with each other's long-term Ed25519 key; transient
//! pair-setup, with a PIN everyone knows, only gives a session. Pair-verify
//! then proves both sides on each connection, for a session's secret.

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::Sha512;
use subtle::ConstantTimeEq;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::crypto::{self, message_nonce};
use crate::error::{Error, Result};
use crate::srp::{self, Group};
use crate::tlv8::{Tag, Tlv};

/// The PIN of transient pairing, which receivers open to everyone take.
pub const TRANSIENT_PIN: &str = "3939";

const USERNAME: &[u8] = b"Pair-Setup";
/// Asks for transient pairing, in M1's flags.
const TRANSIENT: u8 = 0x10;

const WRONG_PIN: &str = "the PIN is wrong";
const FORGOTTEN: &str = "it doesn't know this computer any more; pair again";

/// Whether `proof` is the one `expected`, compared in the same time however
/// much of it is right, so how long it takes gives nothing away.
fn proves(proof: &[u8], expected: &[u8]) -> bool {
    proof.ct_eq(expected).into()
}

/// What pairing with a PIN leaves: who we are to the receiver, and how to
/// tell it's the receiver. Kept, to verify on each connection.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    /// Our pairing id, which the receiver keeps.
    pub(crate) id: String,
    /// The seed of our long-term key.
    pub(crate) secret: [u8; 32],
    /// The receiver's pairing id.
    pub(crate) receiver: String,
    /// The receiver's long-term public key.
    pub(crate) receiver_key: [u8; 32],
}

impl Credentials {
    /// As text to keep: four fields in hex, by colons.
    pub fn to_text(&self) -> String {
        [
            hex(self.id.as_bytes()),
            hex(&self.secret),
            hex(self.receiver.as_bytes()),
            hex(&self.receiver_key),
        ]
        .join(":")
    }

    /// From what `to_text` gave.
    pub fn from_text(text: &str) -> Option<Self> {
        let mut fields = text.trim().split(':').map(unhex);
        let id = String::from_utf8(fields.next()??).ok()?;
        let secret = fields.next()??.try_into().ok()?;
        let receiver = String::from_utf8(fields.next()??).ok()?;
        let receiver_key = fields.next()??.try_into().ok()?;
        fields.next().is_none().then_some(Self {
            id,
            secret,
            receiver,
            receiver_key,
        })
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("id", &self.id)
            .field("receiver", &self.receiver)
            .finish_non_exhaustive()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// A pairing id: random, as a UUID is written.
fn new_id() -> String {
    let bytes: [u8; 16] = rand::random();
    let hex = hex(&bytes).to_uppercase();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// A received message, checked to be the step expected and not an error;
/// `refused` says what failing to authenticate means at this step.
fn received(bytes: &[u8], state: u8, refused: &str) -> Result<Tlv> {
    let tlv = Tlv::decode(bytes)?;
    if let Some(code) = tlv.get(Tag::Error).and_then(|code| code.first()) {
        return Err(Error::pairing(match code {
            2 => refused,
            3 => "it asks to wait before trying again",
            4 => "it has as many pairings as it keeps",
            5 => "too many tries",
            6 => "it isn't taking pairings",
            7 => "it's pairing with something else",
            _ => "it refused",
        }));
    }
    if tlv.state() != Some(state) {
        return Err(Error::protocol(format!(
            "pairing step {:?} where {state} was expected",
            tlv.state()
        )));
    }
    Ok(tlv)
}

fn key32(bytes: &[u8]) -> Result<[u8; 32]> {
    bytes
        .try_into()
        .map_err(|_| Error::protocol("a key that isn't 32 bytes"))
}

fn verify(key: &[u8; 32], message: &[u8], signature: &[u8]) -> Result<()> {
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| Error::protocol("a signature that isn't 64 bytes"))?;
    VerifyingKey::from_bytes(key)
        .and_then(|key| key.verify(message, &Signature::from_bytes(&signature)))
        .map_err(|_| Error::pairing("its signature doesn't match"))
}

/// Pair-setup, from our side: M1, M3 and M5 out, M2, M4 and M6 in.
pub struct Setup {
    pin: String,
    transient: bool,
    client: srp::Client,
    session: Option<srp::Session>,
    id: String,
    signing: SigningKey,
}

impl Setup {
    /// With the PIN a receiver shows.
    pub fn new(pin: &str) -> Self {
        Self::start(pin, false)
    }

    /// For a session only, with the PIN everyone knows.
    pub fn transient() -> Self {
        Self::start(TRANSIENT_PIN, true)
    }

    fn start(pin: &str, transient: bool) -> Self {
        Self {
            pin: pin.to_owned(),
            transient,
            client: srp::Client::new(Group::pairing(), &rand::random::<[u8; 32]>()),
            session: None,
            id: new_id(),
            signing: SigningKey::from_bytes(&rand::random()),
        }
    }

    pub fn m1(&self) -> Vec<u8> {
        let tlv = Tlv::new().byte(Tag::Method, 0).byte(Tag::State, 1);
        let tlv = if self.transient {
            tlv.byte(Tag::Flags, TRANSIENT)
        } else {
            tlv
        };
        tlv.encode()
    }

    /// Our public key and proof, from the receiver's salt and public key.
    pub fn m3(&mut self, m2: &[u8]) -> Result<Vec<u8>> {
        let m2 = received(m2, 2, WRONG_PIN)?;
        let session = self.client.session::<Sha512>(
            USERNAME,
            self.pin.as_bytes(),
            m2.need(Tag::Salt)?,
            m2.need(Tag::PublicKey)?,
        )?;
        let m3 = Tlv::new()
            .byte(Tag::State, 3)
            .with(Tag::PublicKey, self.client.public())
            .with(Tag::Proof, &session.proof)
            .encode();
        self.session = Some(session);
        Ok(m3)
    }

    /// The session's secret, once the receiver has proven it knows it.
    pub fn m4(&mut self, m4: &[u8]) -> Result<Vec<u8>> {
        let m4 = received(m4, 4, WRONG_PIN)?;
        let session = self.session()?;
        if !proves(m4.need(Tag::Proof)?, &session.server_proof) {
            return Err(Error::pairing("the receiver's proof doesn't match"));
        }
        Ok(session.key.clone())
    }

    /// Who we are, sealed: our id and long-term key, signed.
    pub fn m5(&self) -> Result<Vec<u8>> {
        let key = &self.session()?.key;
        let x = crypto::derive(
            key,
            "Pair-Setup-Controller-Sign-Salt",
            "Pair-Setup-Controller-Sign-Info",
        );
        let public = self.signing.verifying_key().to_bytes();
        let signature = self
            .signing
            .sign(&[&x[..], self.id.as_bytes(), &public].concat());
        let inner = Tlv::new()
            .with(Tag::Identifier, self.id.as_bytes())
            .with(Tag::PublicKey, public)
            .with(Tag::Signature, signature.to_bytes())
            .encode();
        let sealed = crypto::seal(&self.encrypt_key()?, message_nonce("PS-Msg05"), &[], &inner);
        Ok(Tlv::new()
            .byte(Tag::State, 5)
            .with(Tag::EncryptedData, sealed)
            .encode())
    }

    /// The receiver's id and long-term key, checked against its signature.
    pub fn m6(&self, m6: &[u8]) -> Result<Credentials> {
        let m6 = received(m6, 6, WRONG_PIN)?;
        let inner = crypto::open(
            &self.encrypt_key()?,
            message_nonce("PS-Msg06"),
            &[],
            m6.need(Tag::EncryptedData)?,
        )?;
        let inner = Tlv::decode(&inner)?;
        let receiver = inner.need(Tag::Identifier)?;
        let receiver_key = key32(inner.need(Tag::PublicKey)?)?;
        let x = crypto::derive(
            &self.session()?.key,
            "Pair-Setup-Accessory-Sign-Salt",
            "Pair-Setup-Accessory-Sign-Info",
        );
        verify(
            &receiver_key,
            &[&x[..], receiver, &receiver_key].concat(),
            inner.need(Tag::Signature)?,
        )?;
        Ok(Credentials {
            id: self.id.clone(),
            secret: self.signing.to_bytes(),
            receiver: String::from_utf8_lossy(receiver).into_owned(),
            receiver_key,
        })
    }

    fn session(&self) -> Result<&srp::Session> {
        self.session
            .as_ref()
            .ok_or_else(|| Error::protocol("pairing steps out of order"))
    }

    fn encrypt_key(&self) -> Result<[u8; 32]> {
        Ok(crypto::derive(
            &self.session()?.key,
            "Pair-Setup-Encrypt-Salt",
            "Pair-Setup-Encrypt-Info",
        ))
    }
}

/// Pair-verify, from our side: M1 and M3 out, M2 and M4 in.
pub struct Verify {
    credentials: Credentials,
    secret: StaticSecret,
    public: PublicKey,
    shared: Option<[u8; 32]>,
}

impl Verify {
    pub fn new(credentials: &Credentials) -> Self {
        let secret = StaticSecret::from(rand::random::<[u8; 32]>());
        Self {
            credentials: credentials.clone(),
            public: PublicKey::from(&secret),
            secret,
            shared: None,
        }
    }

    pub fn m1(&self) -> Vec<u8> {
        Tlv::new()
            .byte(Tag::State, 1)
            .with(Tag::PublicKey, self.public.as_bytes())
            .encode()
    }

    /// Checks it is the receiver paired with, and says who we are.
    pub fn m3(&mut self, m2: &[u8]) -> Result<Vec<u8>> {
        let m2 = received(m2, 2, FORGOTTEN)?;
        let theirs = key32(m2.need(Tag::PublicKey)?)?;
        let shared = self
            .secret
            .diffie_hellman(&PublicKey::from(theirs))
            .to_bytes();
        let key = crypto::derive(
            &shared,
            "Pair-Verify-Encrypt-Salt",
            "Pair-Verify-Encrypt-Info",
        );
        let inner = crypto::open(
            &key,
            message_nonce("PV-Msg02"),
            &[],
            m2.need(Tag::EncryptedData)?,
        )?;
        let inner = Tlv::decode(&inner)?;
        let receiver = inner.need(Tag::Identifier)?;
        if receiver != self.credentials.receiver.as_bytes() {
            return Err(Error::pairing("it isn't the receiver paired with"));
        }
        verify(
            &self.credentials.receiver_key,
            &[&theirs[..], receiver, self.public.as_bytes()].concat(),
            inner.need(Tag::Signature)?,
        )?;

        let signing = SigningKey::from_bytes(&self.credentials.secret);
        let id = self.credentials.id.as_bytes();
        let signature = signing.sign(&[self.public.as_bytes(), id, &theirs[..]].concat());
        let inner = Tlv::new()
            .with(Tag::Identifier, id)
            .with(Tag::Signature, signature.to_bytes())
            .encode();
        self.shared = Some(shared);
        Ok(Tlv::new()
            .byte(Tag::State, 3)
            .with(
                Tag::EncryptedData,
                crypto::seal(&key, message_nonce("PV-Msg03"), &[], &inner),
            )
            .encode())
    }

    /// The session's secret, once the receiver has taken who we are.
    pub fn m4(&self, m4: &[u8]) -> Result<[u8; 32]> {
        received(m4, 4, FORGOTTEN)?;
        self.shared
            .ok_or_else(|| Error::protocol("pairing steps out of order"))
    }
}

/// The receiver's side, for the fake one.
#[cfg(any(test, feature = "fake"))]
pub mod receiver {
    use super::*;

    /// A receiver's pairing: its identity, its PIN, and who it has paired.
    pub struct Accessory {
        pub id: String,
        signing: SigningKey,
        pin: String,
        paired: Vec<(Vec<u8>, [u8; 32])>,
        setup: Option<(srp::Server, bool)>,
        session: Option<srp::Session>,
        verify: Option<([u8; 32], [u8; 32], [u8; 32])>,
    }

    /// What a pairing message led to.
    pub enum Outcome {
        /// More to come.
        Going,
        /// A session's secret: transient pairing's, or pair-verify's.
        Secret(Vec<u8>),
    }

    impl Accessory {
        pub fn new(id: &str, pin: &str) -> Self {
            Self {
                id: id.to_owned(),
                signing: SigningKey::from_bytes(&rand::random()),
                pin: pin.to_owned(),
                paired: Vec::new(),
                setup: None,
                session: None,
                verify: None,
            }
        }

        /// The ids of the senders paired with a PIN.
        pub fn paired(&self) -> Vec<String> {
            self.paired
                .iter()
                .map(|(id, _)| String::from_utf8_lossy(id).into_owned())
                .collect()
        }

        fn refuse(state: u8, code: u8) -> Vec<u8> {
            Tlv::new()
                .byte(Tag::State, state)
                .byte(Tag::Error, code)
                .encode()
        }

        /// The answer to a pair-setup message.
        pub fn setup(&mut self, message: &[u8]) -> Result<(Vec<u8>, Outcome)> {
            let tlv = Tlv::decode(message)?;
            match tlv.state() {
                Some(1) => {
                    let transient = tlv
                        .get(Tag::Flags)
                        .and_then(|flags| flags.first())
                        .is_some_and(|flags| flags & TRANSIENT != 0);
                    let pin = if transient { TRANSIENT_PIN } else { &self.pin };
                    let salt: [u8; 16] = rand::random();
                    let server = srp::Server::new::<Sha512>(
                        Group::pairing(),
                        USERNAME,
                        pin.as_bytes(),
                        &salt,
                        &rand::random::<[u8; 32]>(),
                    );
                    let reply = Tlv::new()
                        .byte(Tag::State, 2)
                        .with(Tag::Salt, salt)
                        .with(Tag::PublicKey, server.public())
                        .encode();
                    self.setup = Some((server, transient));
                    Ok((reply, Outcome::Going))
                }
                Some(3) => {
                    let (server, transient) = self
                        .setup
                        .as_ref()
                        .ok_or_else(|| Error::protocol("M3 before M1"))?;
                    let session = server.session::<Sha512>(USERNAME, tlv.need(Tag::PublicKey)?)?;
                    if !proves(tlv.need(Tag::Proof)?, &session.proof) {
                        return Ok((Self::refuse(4, 2), Outcome::Going));
                    }
                    let reply = Tlv::new()
                        .byte(Tag::State, 4)
                        .with(Tag::Proof, &session.server_proof)
                        .encode();
                    let outcome = if *transient {
                        Outcome::Secret(session.key.clone())
                    } else {
                        Outcome::Going
                    };
                    self.session = Some(session);
                    Ok((reply, outcome))
                }
                Some(5) => {
                    let key = &self
                        .session
                        .as_ref()
                        .ok_or_else(|| Error::protocol("M5 before M3"))?
                        .key;
                    let encrypt =
                        crypto::derive(key, "Pair-Setup-Encrypt-Salt", "Pair-Setup-Encrypt-Info");
                    let inner = Tlv::decode(&crypto::open(
                        &encrypt,
                        message_nonce("PS-Msg05"),
                        &[],
                        tlv.need(Tag::EncryptedData)?,
                    )?)?;
                    let id = inner.need(Tag::Identifier)?;
                    let public = key32(inner.need(Tag::PublicKey)?)?;
                    let x = crypto::derive(
                        key,
                        "Pair-Setup-Controller-Sign-Salt",
                        "Pair-Setup-Controller-Sign-Info",
                    );
                    if verify(
                        &public,
                        &[&x[..], id, &public].concat(),
                        inner.need(Tag::Signature)?,
                    )
                    .is_err()
                    {
                        return Ok((Self::refuse(6, 2), Outcome::Going));
                    }
                    self.paired.push((id.to_vec(), public));

                    let x = crypto::derive(
                        key,
                        "Pair-Setup-Accessory-Sign-Salt",
                        "Pair-Setup-Accessory-Sign-Info",
                    );
                    let ours = self.signing.verifying_key().to_bytes();
                    let signature = self
                        .signing
                        .sign(&[&x[..], self.id.as_bytes(), &ours].concat());
                    let inner = Tlv::new()
                        .with(Tag::Identifier, self.id.as_bytes())
                        .with(Tag::PublicKey, ours)
                        .with(Tag::Signature, signature.to_bytes())
                        .encode();
                    let reply = Tlv::new()
                        .byte(Tag::State, 6)
                        .with(
                            Tag::EncryptedData,
                            crypto::seal(&encrypt, message_nonce("PS-Msg06"), &[], &inner),
                        )
                        .encode();
                    Ok((reply, Outcome::Going))
                }
                state => Err(Error::protocol(format!("pair-setup step {state:?}"))),
            }
        }

        /// The answer to a pair-verify message.
        pub fn verify(&mut self, message: &[u8]) -> Result<(Vec<u8>, Outcome)> {
            let tlv = Tlv::decode(message)?;
            match tlv.state() {
                Some(1) => {
                    let theirs = key32(tlv.need(Tag::PublicKey)?)?;
                    let secret = StaticSecret::from(rand::random::<[u8; 32]>());
                    let ours = PublicKey::from(&secret).to_bytes();
                    let shared = secret.diffie_hellman(&PublicKey::from(theirs)).to_bytes();
                    let signature = self
                        .signing
                        .sign(&[&ours[..], self.id.as_bytes(), &theirs].concat());
                    let inner = Tlv::new()
                        .with(Tag::Identifier, self.id.as_bytes())
                        .with(Tag::Signature, signature.to_bytes())
                        .encode();
                    let key = crypto::derive(
                        &shared,
                        "Pair-Verify-Encrypt-Salt",
                        "Pair-Verify-Encrypt-Info",
                    );
                    let reply = Tlv::new()
                        .byte(Tag::State, 2)
                        .with(Tag::PublicKey, ours)
                        .with(
                            Tag::EncryptedData,
                            crypto::seal(&key, message_nonce("PV-Msg02"), &[], &inner),
                        )
                        .encode();
                    self.verify = Some((ours, theirs, shared));
                    Ok((reply, Outcome::Going))
                }
                Some(3) => {
                    let (ours, theirs, shared) =
                        self.verify.ok_or_else(|| Error::protocol("M3 before M1"))?;
                    let key = crypto::derive(
                        &shared,
                        "Pair-Verify-Encrypt-Salt",
                        "Pair-Verify-Encrypt-Info",
                    );
                    let inner = Tlv::decode(&crypto::open(
                        &key,
                        message_nonce("PV-Msg03"),
                        &[],
                        tlv.need(Tag::EncryptedData)?,
                    )?)?;
                    let id = inner.need(Tag::Identifier)?;
                    let known = self
                        .paired
                        .iter()
                        .find(|(paired, _)| paired == id)
                        .map(|(_, key)| *key);
                    let proven = known.is_some_and(|public| {
                        verify(
                            &public,
                            &[&theirs[..], id, &ours].concat(),
                            inner.need(Tag::Signature).unwrap_or_default(),
                        )
                        .is_ok()
                    });
                    if !proven {
                        return Ok((Self::refuse(4, 2), Outcome::Going));
                    }
                    let reply = Tlv::new().byte(Tag::State, 4).encode();
                    Ok((reply, Outcome::Secret(shared.to_vec())))
                }
                state => Err(Error::protocol(format!("pair-verify step {state:?}"))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::receiver::{Accessory, Outcome};
    use super::*;

    fn secret(outcome: Outcome) -> Vec<u8> {
        match outcome {
            Outcome::Secret(secret) => secret,
            Outcome::Going => panic!("no secret yet"),
        }
    }

    fn going(outcome: Outcome) {
        assert!(matches!(outcome, Outcome::Going), "a secret too soon");
    }

    fn pair(accessory: &mut Accessory, pin: &str) -> Result<Credentials> {
        let mut setup = Setup::new(pin);
        let (m2, outcome) = accessory.setup(&setup.m1())?;
        going(outcome);
        let (m4, outcome) = accessory.setup(&setup.m3(&m2)?)?;
        going(outcome);
        setup.m4(&m4)?;
        let (m6, outcome) = accessory.setup(&setup.m5()?)?;
        going(outcome);
        setup.m6(&m6)
    }

    #[test]
    fn pairing_with_the_pin_then_verifying_gives_both_sides_the_same_secret() {
        let mut accessory = Accessory::new("AA:BB:CC:DD:EE:FF", "1234");
        let credentials = pair(&mut accessory, "1234").unwrap();
        assert_eq!(credentials.receiver, "AA:BB:CC:DD:EE:FF");
        assert_eq!(accessory.paired(), std::slice::from_ref(&credentials.id));

        let mut verify = Verify::new(&credentials);
        let (m2, outcome) = accessory.verify(&verify.m1()).unwrap();
        going(outcome);
        let (m4, outcome) = accessory.verify(&verify.m3(&m2).unwrap()).unwrap();
        assert_eq!(verify.m4(&m4).unwrap().to_vec(), secret(outcome));
    }

    #[test]
    fn a_wrong_pin_is_refused_as_such() {
        let mut accessory = Accessory::new("AA:BB:CC:DD:EE:FF", "1234");
        let err = pair(&mut accessory, "4321").unwrap_err();
        assert!(err.to_string().contains("PIN is wrong"), "{err}");
        assert!(accessory.paired().is_empty());
    }

    /// `message` with the first byte of its proof changed.
    fn forged(message: &[u8], state: u8) -> Vec<u8> {
        let tlv = Tlv::decode(message).unwrap();
        let mut proof = tlv.need(Tag::Proof).unwrap().to_vec();
        proof[0] ^= 1;
        let mut forged = Tlv::new().byte(Tag::State, state);
        if let Some(public) = tlv.get(Tag::PublicKey) {
            forged = forged.with(Tag::PublicKey, public);
        }
        forged.with(Tag::Proof, proof).encode()
    }

    #[test]
    fn a_receiver_whose_proof_is_wrong_is_refused() {
        let mut accessory = Accessory::new("AA:BB:CC:DD:EE:FF", "1234");
        let mut setup = Setup::new("1234");
        let (m2, _) = accessory.setup(&setup.m1()).unwrap();
        let (m4, _) = accessory.setup(&setup.m3(&m2).unwrap()).unwrap();
        let err = setup.m4(&forged(&m4, 4)).unwrap_err();
        assert!(err.to_string().contains("proof doesn't match"), "{err}");
        let tlv = Tlv::decode(&m4).unwrap();
        let proof = tlv.need(Tag::Proof).unwrap();
        let short = Tlv::new()
            .byte(Tag::State, 4)
            .with(Tag::Proof, &proof[..proof.len() - 1])
            .encode();
        assert!(setup.m4(&short).is_err(), "nor is one cut short");
        assert!(setup.m4(&m4).is_ok(), "the real one is taken");
    }

    #[test]
    fn a_sender_whose_proof_is_wrong_is_refused() {
        let mut accessory = Accessory::new("AA:BB:CC:DD:EE:FF", "1234");
        let mut setup = Setup::new("1234");
        let (m2, _) = accessory.setup(&setup.m1()).unwrap();
        let (m4, outcome) = accessory
            .setup(&forged(&setup.m3(&m2).unwrap(), 3))
            .unwrap();
        going(outcome);
        assert!(Tlv::decode(&m4).unwrap().get(Tag::Error).is_some());
        let err = setup.m4(&m4).unwrap_err();
        assert!(err.to_string().contains("PIN is wrong"), "{err}");
    }

    #[test]
    fn transient_pairing_gives_a_secret_after_four_steps() {
        let mut accessory = Accessory::new("AA:BB:CC:DD:EE:FF", "1234");
        let mut setup = Setup::transient();
        let (m2, outcome) = accessory.setup(&setup.m1()).unwrap();
        going(outcome);
        let (m4, outcome) = accessory.setup(&setup.m3(&m2).unwrap()).unwrap();
        assert_eq!(setup.m4(&m4).unwrap(), secret(outcome));
        assert!(accessory.paired().is_empty(), "nothing kept");
    }

    #[test]
    fn verifying_with_another_receivers_credentials_fails() {
        let mut ours = Accessory::new("AA:BB:CC:DD:EE:FF", "1234");
        let mut other = Accessory::new("11:22:33:44:55:66", "1234");
        let credentials = pair(&mut ours, "1234").unwrap();
        pair(&mut other, "1234").unwrap();
        let mut verify = Verify::new(&credentials);
        let (m2, _) = other.verify(&verify.m1()).unwrap();
        assert!(verify.m3(&m2).is_err());
    }

    #[test]
    fn a_receiver_doesnt_verify_a_sender_it_never_paired() {
        let mut accessory = Accessory::new("AA:BB:CC:DD:EE:FF", "1234");
        let mut credentials = pair(&mut accessory, "1234").unwrap();
        credentials.id = "someone else".into();
        let mut verify = Verify::new(&credentials);
        let (m2, _) = accessory.verify(&verify.m1()).unwrap();
        let (m4, outcome) = accessory.verify(&verify.m3(&m2).unwrap()).unwrap();
        going(outcome);
        let err = verify.m4(&m4).unwrap_err();
        assert!(err.to_string().contains("pair again"), "{err}");
    }

    #[test]
    fn credentials_come_back_from_their_text() {
        let mut accessory = Accessory::new("AA:BB:CC:DD:EE:FF", "1234");
        let credentials = pair(&mut accessory, "1234").unwrap();
        let text = credentials.to_text();
        assert_eq!(Credentials::from_text(&text), Some(credentials));
        assert_eq!(Credentials::from_text("ab:cd"), None);
        assert_eq!(Credentials::from_text(&format!("{text}:00")), None);
    }
}
