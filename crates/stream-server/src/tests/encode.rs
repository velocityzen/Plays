use super::*;

/// An encoder failing to open (too big a picture, or the hardware's
/// sessions used up) is left alone a while at that size, not for good.
#[test]
fn tries_an_encoder_again_after_it_fails_to_open() {
    let mut probed = Probed::default();
    let start = Instant::now();
    let first = video_encoders(VideoCodec::H264)[0];
    let never = |_: &'static str| -> bool { panic!("tried again") };

    // Too big for it, so none here at that size, for a while.
    let big = (8192, 4320);
    assert_eq!(
        probed.encoder(VideoCodec::H264, big, false, start, |_| false),
        None
    );
    let later = start + Duration::from_secs(10);
    assert_eq!(
        probed.encoder(VideoCodec::H264, big, false, later, never),
        None
    );
    // A size it takes is tried, and finds it; from then on it's had.
    let hd = (1920, 1080);
    assert_eq!(
        probed.encoder(VideoCodec::H264, hd, false, later, |_| true),
        Some(first)
    );
    assert_eq!(
        probed.encoder(VideoCodec::H264, (640, 360), false, later, never),
        Some(first)
    );

    // Busy at first, and free a minute on.
    let first = video_encoders(VideoCodec::Hevc)[0];
    assert_eq!(
        probed.encoder(VideoCodec::Hevc, hd, true, start, |_| false),
        None
    );
    let soon = start + Duration::from_secs(30);
    assert_eq!(
        probed.encoder(VideoCodec::Hevc, hd, true, soon, never),
        None
    );
    let minute = start + RETRY_AFTER;
    assert_eq!(
        probed.encoder(VideoCodec::Hevc, hd, true, minute, |_| true),
        Some(first)
    );
}
