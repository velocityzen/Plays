use super::*;

#[test]
fn a_paired_device_is_listed_under_the_output_it_will_have() {
    let id = output_id("00-00-5e-00-53-af");
    assert_eq!(
        id, "coreaudio:00-00-5E-00-53-AF:output",
        "as macOS writes it"
    );
    assert_eq!(address(&id), Some("00-00-5E-00-53-AF"));
    assert_eq!(address("coreaudio:BuiltInSpeakerDevice"), None);
    assert_eq!(address("coreaudio:00-00-5E-00-53:output"), None);
    assert_eq!(address("coreaudio:00-00-5E-00-53-01:input"), None);
    assert_eq!(address("coreaudio:00-00-5E-00-53-zz:output"), None);
    assert_eq!(address("airplay:00-00-5E-00-53-01"), None);
}

#[test]
fn lists_those_not_connected() {
    let paired = [
        Paired {
            address: "00-00-5e-00-53-01".into(),
            name: "Radio".into(),
        },
        Paired {
            address: "00-00-5e-00-53-02".into(),
            name: "Pods".into(),
        },
    ];
    // The second is connected: macOS lists its output.
    let outputs = [
        DeviceInfo {
            id: "coreaudio:BuiltInSpeakerDevice".into(),
            name: "MacBook Pro Speakers".into(),
            bluetooth: false,
        },
        DeviceInfo {
            id: "coreaudio:00-00-5E-00-53-02:output".into(),
            name: "Pods".into(),
            bluetooth: true,
        },
    ];
    assert_eq!(
        not_connected(&paired, &outputs),
        [DeviceInfo {
            id: "coreaudio:00-00-5E-00-53-01:output".into(),
            name: "Radio".into(),
            bluetooth: true,
        }]
    );
}

#[test]
fn those_that_play_sound_are_headphones_and_speakers() {
    assert!(plays_sound(0x04, 0x06), "headphones");
    assert!(plays_sound(0x04, 0x01), "a headset");
    assert!(plays_sound(0x04, 0x05), "a speaker");
    assert!(!plays_sound(0x04, 0x04), "a microphone");
    assert!(!plays_sound(0x04, 0x09), "a set-top box");
    assert!(!plays_sound(0x01, 0x06), "a computer");
}

#[cfg(target_os = "macos")]
#[test]
fn leaves_bluetooth_be_outside_the_app() {
    use super::mac::{connect, may_use, paired};

    // The test binary's Info.plist, which it hasn't, says nothing
    // of Bluetooth: macOS would stop it.
    assert!(!may_use());
    assert!(paired().is_empty());
    assert!(connect("00-00-5e-00-53-01").is_err());
}
