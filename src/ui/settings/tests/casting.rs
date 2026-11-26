use super::*;

fn device(kind: &'static str, id: &str, name: &str, shows: Shows) -> Device {
    Device {
        kind,
        id: id.into(),
        name: name.into(),
        shows,
        address: (std::net::Ipv4Addr::LOCALHOST, 0).into(),
    }
}

fn output(id: &str, name: &str) -> DeviceInfo {
    DeviceInfo {
        id: id.into(),
        name: name.into(),
        bluetooth: false,
    }
}

fn listed(found: &[Found]) -> Vec<(&str, &str, Shows)> {
    found
        .iter()
        .map(|device| (device.name.as_str(), device.kind, device.shows))
        .collect()
}

#[test]
fn a_device_found_by_two_protocols_is_listed_with_each() {
    let devices = [
        device(airplay::KIND, "tv", "Beamer", Shows::Video),
        device("DLNA", "uuid-1", "Phantom", Shows::Sound),
    ];
    // This computer's speakers, the Phantom over AirPlay, and the Apple
    // TV, which plays sound too.
    let outputs = [
        output("BuiltInSpeakerDevice", "MacBook Pro Speakers"),
        output("airplay:ph", "Phantom"),
        output("airplay:tv", "Beamer"),
    ];
    assert_eq!(
        listed(&found_devices(&devices, &outputs, true)),
        [
            ("Beamer", airplay::KIND, Shows::Video),
            ("Phantom", airplay::KIND, Shows::Sound),
            ("Phantom", "DLNA", Shows::Sound),
        ]
    );
    assert_eq!(
        listed(&found_devices(&devices, &outputs, false)),
        [
            ("Beamer", airplay::KIND, Shows::Video),
            ("Phantom", "DLNA", Shows::Sound),
        ],
        "AirPlay's speakers, while AirPlay looks"
    );
}

#[test]
fn devices_found_say_what_they_show() {
    assert_eq!(shows(Shows::Video), "Video");
    assert_eq!(shows(Shows::Sound), "Sound only");
}
