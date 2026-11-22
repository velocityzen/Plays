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
