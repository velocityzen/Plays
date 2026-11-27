use super::*;

fn device(kind: &'static str, id: &str, name: &str, ip: [u8; 4]) -> Device {
    Device {
        kind,
        id: id.into(),
        name: name.into(),
        shows: Shows::Video,
        address: SocketAddr::from((ip, 1900)),
    }
}

#[test]
fn devices_of_a_protocol_sharing_a_name_are_told_apart_by_address() {
    let mut devices = vec![
        device("DLNA", "uuid:tv", "Living Room TV", [192, 168, 1, 20]),
        device("DLNA", "uuid:other", "Living Room TV", [192, 168, 1, 66]),
        device("AirPlay", "tv", "Living Room TV", [192, 168, 1, 20]),
        device("DLNA", "uuid:den", "Den", [192, 168, 1, 30]),
    ];
    told_apart(&mut devices);
    let names: Vec<&str> = devices.iter().map(|device| device.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Living Room TV (192.168.1.20)",
            "Living Room TV (192.168.1.66)",
            "Living Room TV",
            "Den",
        ]
    );
}
