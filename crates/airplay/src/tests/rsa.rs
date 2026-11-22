use super::*;

#[test]
fn the_airport_key_is_2048_bits() {
    let airport = PublicKey::airport();
    assert_eq!(airport.size(), 256);
    assert_eq!(airport.encrypt(&[7; 16]).len(), 256);
}

#[test]
fn what_is_encrypted_decrypts_with_the_private_half() {
    let key = PrivateKey::fake();
    let message: [u8; 16] = rand::random();
    let first = key.public().encrypt(&message);
    let second = key.public().encrypt(&message);
    assert_ne!(first, second, "a fresh seed each time");
    assert_eq!(key.decrypt(&first).as_deref(), Some(&message[..]));
    assert_eq!(key.decrypt(&second).as_deref(), Some(&message[..]));
    let mut broken = first.clone();
    broken[100] ^= 1;
    assert_eq!(key.decrypt(&broken), None);
    // Not for this key.
    assert_eq!(key.decrypt(&PublicKey::airport().encrypt(&message)), None);
}
