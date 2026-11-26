use super::*;
use crate::fake::{FakeReceiver, Kind};

/// A mode as `HDRSupportedModes` lists it, up to 4K at 60.
fn mode(name: &str) -> String {
    format!(
        "<dict><key>HDRMode</key><string>{name}</string>\
         <key>highestPlayableFrameRate</key><integer>60</integer>\
         <key>highestPlayableHeight</key><integer>2160</integer>\
         <key>highestPlayableWidth</key><integer>3840</integer>\
         <key>receiverHDRCapability</key><string>4k60</string></dict>"
    )
}

fn info(display: &str) -> Dictionary {
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
         <key>model</key><string>VisionMaster</string>\
         <key>displays</key><array><dict>{display}</dict></array></dict></plist>"
    );
    Value::from_reader_xml(xml.as_bytes())
        .unwrap()
        .into_dictionary()
        .unwrap()
}

/// What a Google TV projector with AirPlay built in says of its screen:
/// showing 1080p now, 4K at most, and every HDR.
fn projector() -> Dictionary {
    info(&format!(
        "<key>HDRInfo</key><dict><key>HDRMode</key><string>DoVi</string>\
         <key>codecStrings</key><array><string>dvhe.04.09</string>\
         <string>dvhe.05.09</string><string>hev1.08.09</string>\
         <string>avc3.09.09</string><string>dvh1.05.06</string></array>\
         <key>highestPlayableAverageBitRate</key><integer>19000000</integer>\
         <key>type</key><integer>3</integer></dict>\
         <key>HDRSupportedModes</key><array>{}{}{}{}{}</array>\
         <key>heightPixels</key><integer>1080</integer>\
         <key>heightPixelsMax</key><integer>2160</integer>\
         <key>maxFPS</key><integer>60</integer>\
         <key>widthPixels</key><integer>1920</integer>\
         <key>widthPixelsMax</key><integer>3840</integer>",
        mode("SDR"),
        mode("HDR"),
        mode("HDR10"),
        mode("DoVi"),
        mode("HLG"),
    ))
}

#[test]
fn a_tv_with_airplay_built_in_lists_the_hdr_it_shows_up_to_its_largest_picture() {
    let screen = Screen::from_info(&projector()).unwrap();
    assert_eq!(
        screen.max_size,
        Some((3840, 2160)),
        "not the 1080p it shows now"
    );
    assert_eq!(screen.hdr, [Hdr::Hdr10, Hdr::DolbyVision, Hdr::Hlg]);
}

#[test]
fn the_hdr_it_shows_now_counts_too_and_sdr_or_hdr_alone_name_none() {
    let now = info(
        "<key>HDRInfo</key><dict><key>HDRMode</key><string>HLG</string></dict>\
         <key>HDRSupportedModes</key><array><dict><key>HDRMode</key><string>HDR10</string></dict></array>",
    );
    assert_eq!(Screen::from_info(&now).unwrap().hdr, [Hdr::Hdr10, Hdr::Hlg]);
    let sdr = info(&format!(
        "<key>HDRSupportedModes</key><array>{}{}{}</array>",
        mode("SDR"),
        mode("HDR"),
        mode("HDR12"),
    ));
    assert!(Screen::from_info(&sdr).unwrap().hdr.is_empty());
}

#[test]
fn a_screen_that_says_no_largest_picture_plays_the_one_it_shows() {
    let shown = info(
        "<key>widthPixels</key><integer>1920</integer><key>heightPixels</key><integer>1080</integer>\
         <key>widthPixelsMax</key><integer>0</integer><key>heightPixelsMax</key><integer>0</integer>",
    );
    assert_eq!(
        Screen::from_info(&shown).unwrap().max_size,
        Some((1920, 1080))
    );
    let unsaid = Screen::from_info(&info("")).unwrap();
    assert_eq!(unsaid.max_size, None);
    assert!(unsaid.hdr.is_empty());
}

#[test]
fn a_receiver_without_a_display_says_nothing_of_one() {
    let mut speaker = projector();
    speaker.remove("displays");
    assert_eq!(Screen::from_info(&speaker), None);
    speaker.insert("displays".into(), Value::Array(Vec::new()));
    assert_eq!(Screen::from_info(&speaker), None);
}

#[test]
fn a_tv_is_asked_before_pairing_and_one_that_doesnt_answer_says_nothing() {
    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    assert_eq!(Screen::ask(&tv.device()), None);
    tv.state().info = Some(projector());
    let screen = Screen::ask(&tv.device()).unwrap();
    assert_eq!(screen.hdr, [Hdr::Hdr10, Hdr::DolbyVision, Hdr::Hlg]);
    let asked = tv.state().requests("GET", "/info").len();
    assert_eq!(asked, 2);
    assert!(!tv.state().pin_shown, "it isn't paired with for it");
}

#[test]
fn a_screen_is_asked_at_the_first_address_airplay_answers_at() {
    let tv = FakeReceiver::start(Kind::Tv { pin: "1234" });
    tv.state().info = Some(projector());
    let nothing = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .unwrap();
    let at = tv.device().addresses[0];
    let screen = Screen::at(&[nothing, at]).unwrap();
    assert_eq!(screen.max_size, Some((3840, 2160)));
    assert_eq!(Screen::at(&[nothing]), None);
    assert_eq!(Screen::PORT, 7000, "AirPlay's, as receivers list it");
}
