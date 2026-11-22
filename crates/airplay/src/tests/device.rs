use super::*;

fn record(instance: &str, port: u16, txt: &str) -> Record {
    Record {
        instance: instance.into(),
        // Documentation's, which no computer the tests run on has.
        addresses: vec![
            "2001:db8::20".parse().unwrap(),
            "192.0.2.20".parse().unwrap(),
        ],
        port,
        txt: txt
            .split(' ')
            .filter_map(|pair| pair.split_once('='))
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect(),
    }
}

// Records as an Apple TV 4K and a speaker send them, their ids and keys
// made up.
pub fn apple_tv() -> (Record, Record) {
    (
        record(
            "Living Room",
            7000,
            "acl=0 deviceid=02:11:22:33:44:55 features=0x4A7FDFD5,0x3C177FDE \
                 flags=0x644 model=AppleTV11,1 protovers=1.1 srcvers=1005.13.2 osvers=27.2.1 \
                 pk=1111111111111111111111111111111111111111111111111111111111111111",
        ),
        record(
            "021122334455@Living Room",
            7000,
            "cn=0,1,2,3 da=true et=0,3,5 ft=0x4A7FDFD5,0x3C177FDE sf=0x644 md=0,1,2 \
                 am=AppleTV11,1 tp=UDP vn=65537 vs=1005.13.2",
        ),
    )
}

pub fn speaker() -> (Record, Record) {
    (
        record(
            "Kitchen",
            7000,
            "acl=0 deviceid=06:AA:BB:CC:DD:EE features=0x445F8A00,0x1C340 flags=0x4 \
                 model=Phantom manufacturer=Devialet protovers=1.1 srcvers=366.0 \
                 pk=2222222222222222222222222222222222222222222222222222222222222222",
        ),
        record(
            "06AABBCCDDEE@Kitchen",
            7000,
            "cn=0,1 da=true et=0,4 ft=0x445F8A00,0x1C340 md=0,1,2 sf=0x4 tp=UDP \
                 vn=65537 vs=366.0",
        ),
    )
}

#[test]
fn reads_the_features_low_word_first() {
    let features = Features::parse("0x445F8A00,0x1C340").unwrap();
    assert_eq!(features.0, 0x1C340_445F8A00);
    assert!(features.has(Features::AUDIO));
    assert!(!features.has(Features::VIDEO));
    assert!(features.has(Features::UNIFIED_MEDIA_CONTROL));
    assert_eq!(Features::parse("0x5A7FFFF7").unwrap().0, 0x5A7FFFF7);
    assert_eq!(Features::parse("0x1,0x2").unwrap().0, 0x2_0000_0001);
    assert_eq!(Features::parse("nonsense"), None);
}

#[test]
fn an_apple_tv_shows_video_and_pairs_with_a_pin() {
    let (airplay, raop) = apple_tv();
    let tv = Device::from_records(Some(&airplay), Some(&raop)).unwrap();
    assert_eq!(tv.id, "021122334455");
    assert_eq!(tv.name, "Living Room");
    assert_eq!(tv.model, "AppleTV11,1");
    let addresses: Vec<SocketAddr> = ["192.0.2.20:7000", "[2001:db8::20]:7000"]
        .iter()
        .map(|address| address.parse().unwrap())
        .collect();
    assert_eq!(tv.addresses, addresses);
    assert!(tv.shows_video());
    assert!(tv.plays_audio());
    assert!(tv.airplay2());
    assert_eq!(tv.pairing(), Pairing::Pin);
    assert_eq!(tv.codecs, [0, 1, 2, 3]);
    assert_eq!(tv.public_key, Some([0x11; 32]));
}

#[test]
fn a_speaker_plays_only_sound_and_pairs_for_the_session() {
    let (airplay, raop) = speaker();
    let speaker = Device::from_records(Some(&airplay), Some(&raop)).unwrap();
    assert_eq!(speaker.id, "06AABBCCDDEE");
    assert_eq!(speaker.manufacturer.as_deref(), Some("Devialet"));
    assert!(!speaker.shows_video());
    assert!(speaker.plays_audio());
    assert_eq!(speaker.pairing(), Pairing::Transient);
    assert_eq!(speaker.encryptions, [0, 4]);
}

#[test]
fn a_receiver_with_only_its_raop_record_is_found_too() {
    let raop = record(
        "0A0B0C0D0E0F@Study",
        5000,
        "cn=0,1 et=0,1 ft=0x200 sf=0x0 am=AirPort10,115 tp=UDP",
    );
    let old = Device::from_records(None, Some(&raop)).unwrap();
    assert_eq!(old.id, "0A0B0C0D0E0F");
    assert_eq!(old.name, "Study");
    assert_eq!(old.addresses[0].port(), 5000);
    assert!(!old.airplay2());
    assert_eq!(old.pairing(), Pairing::None);
}

#[test]
fn an_airplay_1_speaker_that_doesnt_say_its_features_plays_sound() {
    let raop = record(
        "0A0B0C0D0E10@Hall",
        5000,
        "txtvers=1 ch=2 cn=0,1 et=1 sr=44100 ss=16 tp=UDP vs=105.1 am=AirPort4,107",
    );
    let old = Device::from_records(None, Some(&raop)).unwrap();
    assert!(old.plays_audio());
    assert!(!old.shows_video());
    assert!(!old.airplay2());
    assert_eq!(old.encryptions, [1]);
    // An AirPlay record says what it does, or isn't a receiver.
    let (mut airplay, _) = apple_tv();
    airplay.txt.remove("features");
    assert_eq!(Device::from_records(Some(&airplay), None), None);
}

#[test]
fn a_receiver_is_at_any_of_its_addresses() {
    let (airplay, raop) = apple_tv();
    let tv = Device::from_records(Some(&airplay), Some(&raop)).unwrap();
    let ip = |text: &str| text.parse::<IpAddr>().unwrap();
    assert!(tv.is_at(&[ip("10.0.0.1"), ip("192.0.2.20")]));
    assert!(tv.is_at(&[ip("2001:db8::20")]));
    assert!(!tv.is_at(&[ip("192.0.2.21"), ip("2001:db8::21")]));
    assert!(!tv.is_at(&[]));
}
