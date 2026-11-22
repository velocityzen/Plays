use super::*;

pub fn record(instance: &str, port: u16, txt: &[(&str, &str)]) -> Record {
    Record {
        instance: instance.into(),
        addresses: vec!["fe80::2".parse().unwrap(), "192.168.1.30".parse().unwrap()],
        port,
        txt: txt
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

// Records as a Chromecast with Google TV, a Nest speaker and a speaker
// group send them, their ids made up.
pub fn google_tv() -> Record {
    record(
        "Chromecast-0a1b2c3d4e5f60718293a4b5c6d7e8f9",
        8009,
        &[
            ("id", "0a1b2c3d4e5f60718293a4b5c6d7e8f9"),
            ("cd", "00112233445566778899AABBCCDDEEFF"),
            ("rm", ""),
            ("ve", "05"),
            ("md", "Chromecast"),
            ("ic", "/setup/icon.png"),
            ("fn", "Living Room TV"),
            ("ca", "463365"),
            ("st", "1"),
            ("bs", "FA8F00000000"),
            ("nf", "1"),
            ("rs", "YouTube"),
        ],
    )
}

pub fn speaker() -> Record {
    record(
        "Google-Nest-Mini-f0e1d2c3b4a5968778695a4b3c2d1e0f",
        8009,
        &[
            ("id", "f0e1d2c3b4a5968778695a4b3c2d1e0f"),
            ("md", "Google Nest Mini"),
            ("fn", "Kitchen speaker"),
            ("ca", "199172"),
            ("st", "0"),
            ("rs", ""),
        ],
    )
}

pub fn group() -> Record {
    record(
        "Google-Cast-Group-11112222333344445555666677778888",
        32187,
        &[
            ("id", "11112222-3333-4444-5555-666677778888"),
            ("md", "Google Cast Group"),
            ("fn", "Downstairs"),
            ("ca", "199204"),
            ("st", "0"),
        ],
    )
}

#[test]
fn a_google_tv_shows_video_and_says_what_its_showing() {
    let tv = Device::from_record(&google_tv()).unwrap();
    assert_eq!(tv.id, "0a1b2c3d4e5f60718293a4b5c6d7e8f9");
    assert_eq!(tv.name, "Living Room TV");
    assert_eq!(tv.model, "Chromecast");
    assert!(tv.shows_video());
    assert!(tv.plays_audio());
    assert!(!tv.is_group());
    assert_eq!(tv.showing.as_deref(), Some("YouTube"));
    let addresses: Vec<SocketAddr> = ["192.168.1.30:8009", "[fe80::2]:8009"]
        .iter()
        .map(|address| address.parse().unwrap())
        .collect();
    assert_eq!(tv.addresses, addresses);
}

#[test]
fn a_speaker_plays_only_sound_and_a_group_is_one() {
    let speaker = Device::from_record(&speaker()).unwrap();
    assert!(!speaker.shows_video());
    assert!(speaker.plays_audio());
    assert!(!speaker.is_group());
    assert_eq!(speaker.showing, None, "an empty rs is nothing");

    let group = Device::from_record(&group()).unwrap();
    assert!(group.is_group());
    assert!(group.plays_audio());
    assert_eq!(group.name, "Downstairs");
    assert_eq!(group.addresses[0].port(), 32187);
}

#[test]
fn a_record_without_an_id_isnt_a_device() {
    let mut record = speaker();
    record.txt.remove("id");
    assert_eq!(Device::from_record(&record), None);
    let mut nameless = speaker();
    nameless.txt.remove("fn");
    assert_eq!(
        Device::from_record(&nameless).unwrap().name,
        nameless.instance
    );
}
