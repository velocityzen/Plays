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
    let premaster = client.premaster::<Sha1>(b"alice", b"password123", &salt, &hex(B_PUBLIC), &u);
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
