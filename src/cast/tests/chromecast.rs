use super::*;

fn video(profile: &Profile) -> Vec<VideoCodec> {
    profile.video.clone()
}

#[test]
fn the_first_chromecasts_get_h264_up_to_1080p_and_their_files() {
    let first = profile("Chromecast", true);
    assert_eq!(video(&first), [VideoCodec::H264, VideoCodec::Vp8]);
    assert_eq!(first.audio, SOUND);
    assert_eq!(first.files, FILES);
    assert_eq!(first.max_size, Some(FULL_HD));
    assert_eq!(first.max_bit_depth, 8);
    assert!(first.hdr.is_empty());
    assert_eq!(first.hls, Some(Segments::Ts));
    assert!(first.fetches_urls);
    assert!(!first.hls_subtitles, "they go beside it");
    assert!(!first.stream);
}

#[test]
fn an_ultra_and_a_google_tv_get_hevc_vp9_and_hdr() {
    let ultra = profile("Chromecast Ultra", true);
    assert_eq!(
        video(&ultra),
        [
            VideoCodec::H264,
            VideoCodec::Hevc,
            VideoCodec::Vp9,
            VideoCodec::Vp8
        ]
    );
    assert_eq!(ultra.max_size, Some(UHD));
    assert_eq!(ultra.max_bit_depth, 10);
    assert_eq!(ultra.hdr, [Hdr::Hdr10]);
    assert_eq!(ultra.hls, Some(Segments::Fmp4));

    let google_tv = profile("Chromecast with Google TV", true);
    assert_eq!(video(&google_tv), video(&ultra));
    assert_eq!(google_tv.max_size, Some(UHD));
    assert_eq!(google_tv.hdr, [Hdr::Hdr10, Hdr::Hlg]);

    let hd = profile("Chromecast HD", true);
    assert_eq!(video(&hd), video(&ultra));
    assert_eq!(hd.max_size, Some(FULL_HD), "the HD one");
    assert_eq!(hd.hdr, [Hdr::Hdr10, Hdr::Hlg]);
    assert_eq!(hd.max_bit_depth, 10);
}

#[test]
fn a_google_tv_streamer_gets_av1_too() {
    let streamer = profile("Google TV Streamer", true);
    assert!(streamer.video.contains(&VideoCodec::Av1));
    assert!(streamer.video.contains(&VideoCodec::Hevc));
    assert_eq!(streamer.max_size, Some(UHD));
    assert_eq!(streamer.hls, Some(Segments::Fmp4));
}

#[test]
fn any_other_tv_gets_h264_and_aac() {
    let other = profile("BRAVIA 4K VH2", true);
    assert_eq!(video(&other), [VideoCodec::H264]);
    assert_eq!(other.audio, [AudioCodec::Aac]);
    assert_eq!(other.files, [Container::Mp4]);
    assert_eq!(other.max_size, Some(FULL_HD));
    assert_eq!(other.hls, Some(Segments::Ts));
    assert!(!other.hls_subtitles);
}

#[test]
fn a_speaker_gets_sound_as_files_or_hls() {
    for model in ["Google Nest Mini", "Google Cast Group", "Chromecast"] {
        let speaker = profile(model, false);
        assert_eq!(speaker.files, FILES, "{model}");
        assert_eq!(speaker.audio, SOUND, "{model}");
        assert_eq!(speaker.hls, Some(Segments::Ts), "{model}");
        assert_eq!(speaker.max_channels, 2, "{model}");
    }
}

#[test]
fn subtitles_look_as_settings_say() {
    let settings = SubtitleStyle {
        font: "Avenir".into(),
        weight: SubtitleWeight::Bold,
        size: 1.5,
        color: Rgb(0xffcc00),
        edge: SubtitleEdge::Shadow,
        edge_color: Rgb(0x102030),
        background: Rgb(0x000000),
        background_opacity: 0.5,
        ..SubtitleStyle::default()
    };
    assert_eq!(
        style(&settings),
        TextTrackStyle {
            foreground: Color([0xff, 0xcc, 0x00, 0xff]),
            background: Color([0, 0, 0, 0x80]),
            edge: Edge::DropShadow,
            edge_color: Color([0x10, 0x20, 0x30, 0xff]),
            font_scale: 1.5,
            font_family: Some("Avenir".into()),
            generic_family: FontFamily::SansSerif,
            bold: true,
            italic: false,
        }
    );
    let plain = style(&SubtitleStyle {
        font: String::new(),
        weight: SubtitleWeight::Medium,
        edge: SubtitleEdge::Outline,
        background_opacity: 0.0,
        ..SubtitleStyle::default()
    });
    assert_eq!(plain.font_family, None, "the device's own");
    assert!(!plain.bold);
    assert_eq!(plain.edge, Edge::Outline);
    assert_eq!(plain.background.0[3], 0, "no box");
    let none = style(&SubtitleStyle {
        edge: SubtitleEdge::None,
        ..SubtitleStyle::default()
    });
    assert_eq!(none.edge, Edge::None);
}
