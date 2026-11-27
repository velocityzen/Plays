use super::*;
use crate::media::Orientation;
use crate::media::details::VideoDetails;
use crate::ui::framing::{ASPECT_RATIOS, CROPS};

/// A video the file says to turn a quarter clockwise, sent with
/// `transfer`.
fn sideways(transfer: Option<&'static str>) -> MediaInfo {
    MediaInfo {
        has_video: true,
        orientation: Orientation::new(1, false),
        video_details: Some(VideoDetails {
            transfer,
            ..VideoDetails::default()
        }),
        ..MediaInfo::default()
    }
}

fn gamma(sdr_gamma: f32, hdr_gamma: f32) -> Settings {
    Settings {
        sdr_gamma,
        hdr_gamma,
        ..Settings::default()
    }
}

#[test]
fn adjusts_the_picture_there_as_it_shows_here() {
    let info = sideways(None);
    let plain = gamma(1.0, 1.0);
    assert_eq!(
        picture_adjustments(&Adjustments::default(), &info, &plain),
        None,
        "as the file is, the device turns it as the file says"
    );
    // Turned and mirrored after the file's own quarter turn.
    let adjusted = Adjustments {
        orientation: Orientation::new(1, true),
        aspect: Some(ASPECT_RATIOS[1]),
        crop: Some(CROPS[0]),
    };
    let there = picture_adjustments(&adjusted, &info, &plain).unwrap();
    let shown = adjusted.orientation.after(info.orientation);
    assert_eq!(
        (there.turns, there.mirrored),
        (shown.clockwise_turns(), shown.is_mirrored())
    );
    assert_eq!((there.turns, there.mirrored), (0, true));
    assert_eq!(there.aspect, Some(f64::from(4.0_f32 / 3.0)));
    assert_eq!(there.crop, Some(1.0));
    assert_eq!(there.gamma, 1.0);
}

#[test]
fn bends_the_picture_there_by_the_gamma_for_its_kind_of_video() {
    let settings = gamma(1.5, 0.8);
    let bent = |info: &MediaInfo| {
        picture_adjustments(&Adjustments::default(), info, &settings).map(|there| there.gamma)
    };
    assert_eq!(bent(&sideways(None)), Some(1.5));
    assert_eq!(bent(&sideways(Some("bt709"))), Some(1.5));
    assert_eq!(bent(&sideways(Some("smpte2084"))), Some(f64::from(0.8_f32)));
    assert_eq!(
        bent(&sideways(Some("arib-std-b67"))),
        Some(f64::from(0.8_f32))
    );
    // Dolby Vision profile 5, whose transfer says nothing, shows here as
    // HDR, reshaped.
    let mut profile_5 = sideways(None);
    profile_5.video_details.as_mut().unwrap().dolby_vision = true;
    assert_eq!(bent(&profile_5), Some(f64::from(0.8_f32)));
    // Turned as the file says once it is bent: the device is sent the
    // picture as it shows.
    let there = picture_adjustments(&Adjustments::default(), &sideways(None), &settings);
    assert_eq!(there.map(|there| there.turns), Some(1));
    // SDR at 1 isn't bent, whatever HDR's gamma.
    let sdr_only = gamma(1.0, 0.8);
    assert_eq!(
        picture_adjustments(&Adjustments::default(), &sideways(None), &sdr_only),
        None
    );
    // Sound has no picture to bend.
    let sound = MediaInfo {
        has_video: false,
        ..sideways(None)
    };
    assert_eq!(
        picture_adjustments(&Adjustments::default(), &sound, &settings),
        None
    );
}
