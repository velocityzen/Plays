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
