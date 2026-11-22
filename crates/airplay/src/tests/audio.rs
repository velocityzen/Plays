use super::*;

#[test]
fn volume_goes_from_minus_30_db_to_0_and_silence_is_minus_144() {
    assert_eq!(volume_db(1.0, false), 0.0);
    assert_eq!(volume_db(0.5, false), -15.0);
    assert_eq!(volume_db(0.0, false), -144.0);
    assert_eq!(volume_db(0.8, true), -144.0);
    assert_eq!(volume_from_db(-15.0), 0.5);
    assert_eq!(volume_from_db(-144.0), 0.0);
}

/// Packets kept to send again, from `first` on, each its header alone.
fn backlog(first: u16, count: u16) -> VecDeque<(u16, Vec<u8>)> {
    (0..count)
        .map(|i| first.wrapping_add(i))
        .map(|seq| (seq, rtp::audio_header(false, seq, 0, 0).to_vec()))
        .collect()
}

/// The sequence numbers of the packets `resends_for` gives.
fn seqs(resent: &[Vec<u8>]) -> Vec<u16> {
    resent
        .iter()
        .map(|packet| {
            let original = rtp::receiver::resent(packet).unwrap();
            rtp::receiver::audio(original).unwrap().0
        })
        .collect()
}

const RECEIVER: [u8; 4] = [192, 168, 1, 20];

fn from_receiver() -> SocketAddr {
    (RECEIVER, 6001).into()
}

fn ask(backlog: &VecDeque<(u16, Vec<u8>)>, first: u16, count: u16) -> Vec<u16> {
    let request = rtp::receiver::resend_request(first, count);
    seqs(&resends_for(
        backlog,
        RECEIVER.into(),
        from_receiver(),
        &request,
    ))
}

#[test]
fn only_the_receiver_is_sent_packets_again() {
    let kept = backlog(100, 10);
    let request = rtp::receiver::resend_request(102, 2);
    let stranger = "192.168.1.99:6001".parse().unwrap();
    assert!(resends_for(&kept, RECEIVER.into(), stranger, &request).is_empty());
    assert_eq!(ask(&kept, 102, 2), [102, 103]);
    assert!(resends_for(&kept, RECEIVER.into(), from_receiver(), b"nonsense").is_empty());
}

#[test]
fn a_request_for_every_packet_gets_at_most_those_kept_in_order() {
    let kept = backlog(64_000, BACKLOG as u16);
    let all: Vec<u16> = kept.iter().map(|(seq, _)| *seq).collect();
    assert_eq!(ask(&kept, 64_000, u16::MAX), all);
    assert_eq!(ask(&kept, 0, u16::MAX), all, "from wherever it starts");
    assert_eq!(
        ask(&kept, 63_990, 15),
        [64_000, 64_001, 64_002, 64_003, 64_004]
    );
    assert_eq!(ask(&kept, 64_999, 0), [] as [u16; 0]);
    assert_eq!(ask(&kept, 63_000, 10), [] as [u16; 0], "long gone");
    assert_eq!(ask(&kept, 2000, 10), [] as [u16; 0], "not yet sent");
}

#[test]
fn sequence_numbers_wrap_around() {
    let kept = backlog(65_530, 10);
    assert_eq!(ask(&kept, 65_534, 4), [65_534, 65_535, 0, 1]);
    assert_eq!(ask(&kept, 2, 5), [2, 3]);
    assert_eq!(ask(&kept, 65_535, 1), [65_535]);
}

#[test]
fn the_sounds_key_is_wrapped_for_the_airport_or_for_a_fake_here() {
    let real = "192.168.1.20:5000".parse().unwrap();
    assert_eq!(speaker_key(real), PublicKey::airport());
    let fake = crate::fake::FakeReceiver::start(crate::fake::Kind::EncryptedSpeaker);
    let address = fake.device().addresses[0];
    assert_eq!(
        speaker_key(address),
        *crate::rsa::PrivateKey::fake().public()
    );
    drop(fake);
    assert_eq!(speaker_key(address), PublicKey::airport());
}
