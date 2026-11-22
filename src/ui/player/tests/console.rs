use super::*;
use crate::media::details::{AudioDetails, ContainerDetails, VideoDetails};

#[test]
fn numbers_read_as_people_write_them() {
    assert_eq!(bit_rate(128_000), "128 kb/s");
    assert_eq!(bit_rate(2_410_000), "2.4 Mb/s");
    assert_eq!(bit_rate(3_000_000), "3 Mb/s");
    assert_eq!(sample_rate(48_000), "48 kHz");
    assert_eq!(sample_rate(44_100), "44.1 kHz");
    assert_eq!(decimals(29.97002997, 3), "29.97");
    assert_eq!(decimals(25.0, 3), "25");
    assert_eq!(time(12.34), "0:12.3");
    assert_eq!(time(3723.0), "1:02:03.0");
    assert_eq!(milliseconds(0.0021), "+2 ms");
    assert_eq!(milliseconds(-0.0001), "+0 ms");
    assert_eq!(milliseconds(-0.003), "-3 ms");
    assert_eq!(seconds(0.05, 2), "+0.05 s");
    assert_eq!(seconds(-0.001, 2), "+0.00 s");
}

fn facts(info: &MediaInfo) -> Facts<'_> {
    Facts {
        source: "/media/clip.mp4".to_owned(),
        info: Some(info),
        stats: PlaybackStats {
            shown_fps: 30.0,
            shown: 300,
            dropped: 2,
            lateness: Some(0.004),
            queued_frames: 8,
            queued_audio: 0.5,
            read_ahead: 1.25,
        },
        position: 12.0,
        duration: Some(60.0),
        picture: Some(("GPU surface", (1280, 720))),
        output_rate: Some(48_000),
        audio_delay: 0.0,
        subtitle_delay: 0.0,
        buffering: false,
        network: false,
    }
}

#[test]
fn shows_the_file_its_streams_and_how_they_play() {
    let info = MediaInfo {
        duration: Some(60.0),
        has_video: true,
        has_audio: true,
        frame_rate: Some(30_000.0 / 1001.0),
        video_codec: Some("h264".to_owned()),
        audio_codec: Some("aac".to_owned()),
        hardware: Some("VideoToolbox"),
        container: ContainerDetails {
            format: "QuickTime / MOV".to_owned(),
            bit_rate: Some(2_500_000),
        },
        video_details: Some(VideoDetails {
            profile: Some("High".to_owned()),
            coded_size: (1280, 720),
            pixel_format: Some("yuv420p"),
            matrix: Some("bt709"),
            primaries: Some("bt709"),
            transfer: Some("bt709"),
            full_range: Some(false),
            bit_rate: Some(2_300_000),
        }),
        audio_details: Some(AudioDetails {
            profile: Some("LC".to_owned()),
            sample_rate: 48_000,
            channels: 2,
            layout: Some("stereo".to_owned()),
            sample_format: Some("fltp"),
            bits: None,
            bit_rate: Some(128_000),
        }),
        ..MediaInfo::default()
    };
    let sections = sections(&facts(&info));
    let rows: Vec<String> = sections
        .iter()
        .flatten()
        .map(|(label, value)| format!("{label}: {value}"))
        .collect();
    assert_eq!(
        rows,
        [
            "File: /media/clip.mp4",
            "Format: QuickTime / MOV · 2.5 Mb/s",
            "Time: 0:12.0 / 1:00.0",
            "Read: 1.2 s",
            "Video: h264 High · 1280×720 · yuv420p · 29.97 fps · 2.3 Mb/s",
            "Color: bt709 · bt709 · bt709 · video range",
            "Decoder: VideoToolbox → 1280×720 GPU surface",
            "Shown: 30 fps · 2 dropped · +4 ms vs clock",
            "Queued: 8 frames",
            "Audio: aac LC · 48 kHz · stereo · fltp · 128 kb/s",
            "Output: 48 kHz stereo · 0.50 s queued",
            "Delay: +0.00 s",
        ]
    );
    assert_eq!(sections.len(), 3);
}

#[test]
fn hdr_and_live_streams_say_so() {
    let info = MediaInfo {
        live: true,
        has_video: true,
        video_codec: Some("hevc".to_owned()),
        video_details: Some(VideoDetails {
            coded_size: (3840, 2160),
            transfer: Some("smpte2084"),
            primaries: Some("bt2020"),
            ..VideoDetails::default()
        }),
        ..MediaInfo::default()
    };
    let mut facts = facts(&info);
    facts.network = true;
    facts.buffering = true;
    let rows: Vec<String> = sections(&facts)
        .into_iter()
        .flatten()
        .map(|(label, value)| format!("{label}: {value}"))
        .collect();
    assert!(rows.contains(&"Time: 0:12.0 · live".to_owned()));
    assert!(rows.contains(&"Downloaded: 1.2 s · buffering".to_owned()));
    assert!(rows.contains(&"Color: HDR PQ · bt2020".to_owned()));
}
