//! The console: what plays in a tile, stream by stream as the file has
//! them, and how playback is going, kept up to date while it shows. `~`
//! shows and hides it on the tile in front.

use gpui::prelude::*;
use gpui::{Context, Div, div};

use super::PlayerView;
use crate::media::decoder::MediaInfo;
use crate::media::player::PlaybackStats;
use crate::media::{Picture, tracks::TrackKind};
use crate::ui::components::Panel;
use crate::ui::theme::console::{FONT, INSET, LABEL_WIDTH, MAX_WIDTH, REFRESH, TOP};
use crate::ui::theme::overlay;

/// A line of the console: what it is about, and what is known.
type Row = (&'static str, String);

impl PlayerView {
    /// Show the console on this tile, or hide it.
    pub fn toggle_console(&mut self, cx: &mut Context<Self>) {
        self.console = match self.console.take() {
            Some(_) => None,
            // What it shows changes while paused too (reading ahead, say),
            // when nothing else redraws the tile.
            None => Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(REFRESH).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            })),
        };
        cx.notify();
    }

    pub(super) fn render_console(&self) -> Option<Div> {
        self.console.as_ref()?;
        let info = self.player.info();
        let facts = Facts {
            source: self.player.source().to_string(),
            info: info.as_ref(),
            stats: self.player.stats(),
            position: self.player.position(),
            duration: self.player.duration().filter(|_| !self.player.is_live()),
            picture: self.player.frame().map(|frame| describe(&frame)),
            output_rate: self.player.mixer().map(|mixer| mixer.sample_rate()),
            audio_delay: self.player.audio_delay(),
            subtitle_delay: self.player.subtitle_delay(),
            buffering: self.player.is_buffering(),
            network: self.player.is_network(),
        };
        let sections = sections(&facts).into_iter().map(|rows| {
            div()
                .flex()
                .flex_col()
                .children(rows.into_iter().map(|(label, value)| {
                    div()
                        .flex()
                        .gap_3()
                        .child(
                            div()
                                .flex_none()
                                .w(LABEL_WIDTH)
                                .text_color(overlay::text_muted())
                                .child(label),
                        )
                        .child(div().min_w_0().child(value))
                }))
        });
        Some(
            div()
                .absolute()
                .top(TOP)
                .left(INSET)
                .max_w(MAX_WIDTH)
                .child(
                    Panel::new()
                        .overlay()
                        .p_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .font_family(FONT)
                        .text_xs()
                        .text_color(overlay::text())
                        .children(sections),
                ),
        )
    }
}

/// Everything the console shows, gathered from the player.
struct Facts<'a> {
    source: String,
    info: Option<&'a MediaInfo>,
    stats: PlaybackStats,
    position: f64,
    duration: Option<f64>,
    /// What the frame showing is in memory, and its size.
    picture: Option<(&'static str, (u32, u32))>,
    output_rate: Option<u32>,
    audio_delay: f64,
    subtitle_delay: f64,
    buffering: bool,
    network: bool,
}

/// What a decoded frame is, as it goes to the GPU, and its size.
fn describe(picture: &Picture) -> (&'static str, (u32, u32)) {
    let kind = match picture {
        Picture::Image(_) => "BGRA image",
        #[cfg(target_os = "macos")]
        Picture::Surface(_) => "GPU surface",
        #[cfg(not(target_os = "macos"))]
        Picture::Yuv(_) => "Y'CbCr planes",
    };
    (kind, picture.size())
}

/// The console's lines, in groups: the file, its video, its sound, its
/// subtitles.
fn sections(facts: &Facts) -> Vec<Vec<Row>> {
    let mut file = vec![("File", facts.source.clone())];
    let Some(info) = facts.info else {
        file.push(("State", "Opening…".to_owned()));
        return vec![file];
    };
    let mut format = info.container.format.clone();
    if let Some(rate) = info.container.bit_rate {
        format = joined([format, bit_rate(rate)]);
    }
    file.push(("Format", format));
    file.push((
        "Time",
        match (info.live, facts.duration) {
            (true, _) => format!("{} · live", time(facts.position)),
            (false, Some(duration)) => format!("{} / {}", time(facts.position), time(duration)),
            (false, None) => time(facts.position),
        },
    ));
    let read_ahead = if facts.stats.read_ahead.is_infinite() {
        "to the end".to_owned()
    } else {
        format!("{:.1} s", facts.stats.read_ahead.max(0.0))
    };
    let read = if facts.buffering {
        format!("{read_ahead} · buffering")
    } else {
        read_ahead
    };
    file.push((if facts.network { "Downloaded" } else { "Read" }, read));
    let mut sections = vec![file];

    if let Some(video) = &info.video_details {
        let mut rows = Vec::new();
        let codec = info.video_codec.clone().unwrap_or_default();
        let (width, height) = video.coded_size;
        rows.push((
            "Video",
            joined(
                [
                    Some(with_profile(codec, video.profile.as_deref())),
                    Some(format!("{width}×{height}")),
                    video.pixel_format.map(str::to_owned),
                    info.frame_rate
                        .map(|fps| format!("{} fps", decimals(fps, 3))),
                    video.bit_rate.map(bit_rate),
                ]
                .into_iter()
                .flatten(),
            ),
        ));
        let light = match video.transfer {
            Some("smpte2084") => Some("HDR PQ".to_owned()),
            Some("arib-std-b67") => Some("HDR HLG".to_owned()),
            other => other.map(str::to_owned),
        };
        let range = video
            .full_range
            .map(|full| if full { "full range" } else { "video range" }.to_owned());
        let color = joined(
            [
                light,
                video.primaries.map(str::to_owned),
                video.matrix.map(str::to_owned),
                range,
            ]
            .into_iter()
            .flatten(),
        );
        if !color.is_empty() {
            rows.push(("Color", color));
        }
        let decoder = info.hardware.unwrap_or("software").to_owned();
        rows.push((
            "Decoder",
            match facts.picture {
                Some((kind, (width, height))) => format!("{decoder} → {width}×{height} {kind}"),
                None => decoder,
            },
        ));
        let stats = facts.stats;
        let mut shown = vec![
            format!("{} fps", decimals(stats.shown_fps, 1)),
            format!("{} dropped", stats.dropped),
        ];
        if let Some(lateness) = stats.lateness {
            shown.push(format!("{} vs clock", milliseconds(lateness)));
        }
        rows.push(("Shown", joined(shown)));
        rows.push((
            "Queued",
            match stats.queued_frames {
                1 => "1 frame".to_owned(),
                frames => format!("{frames} frames"),
            },
        ));
        sections.push(rows);
    }

    if let Some(audio) = &info.audio_details {
        let codec = info.audio_codec.clone().unwrap_or_default();
        let channels = audio
            .layout
            .clone()
            .unwrap_or_else(|| format!("{} channels", audio.channels));
        let mut rows = vec![(
            "Audio",
            joined(
                [
                    Some(with_profile(codec, audio.profile.as_deref())),
                    Some(sample_rate(audio.sample_rate)),
                    Some(channels),
                    audio.sample_format.map(str::to_owned),
                    audio.bit_rate.map(bit_rate),
                ]
                .into_iter()
                .flatten(),
            ),
        )];
        let mut output = facts
            .output_rate
            .map(|rate| vec![format!("{} stereo", sample_rate(rate))])
            .unwrap_or_default();
        output.push(format!("{:.2} s queued", facts.stats.queued_audio));
        rows.push(("Output", joined(output)));
        rows.push(("Delay", seconds(facts.audio_delay, 2)));
        sections.push(rows);
    }

    let subtitles = info
        .subtitle_track
        .and_then(|index| info.tracks.iter().find(|t| t.index == index))
        .filter(|track| track.kind == TrackKind::Subtitle);
    if let Some(track) = subtitles {
        let number = info
            .tracks(TrackKind::Subtitle)
            .position(|t| t.index == track.index)
            .map_or(1, |at| at + 1);
        sections.push(vec![
            ("Subtitles", joined([track.label(number), track.detail()])),
            ("Delay", seconds(facts.subtitle_delay, 1)),
        ]);
    }
    sections
}

fn joined(parts: impl IntoIterator<Item = String>) -> String {
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn with_profile(codec: String, profile: Option<&str>) -> String {
    match profile {
        Some(profile) => format!("{codec} {profile}"),
        None => codec,
    }
}

/// `value` with up to `places` decimals, and none it doesn't need.
fn decimals(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

fn bit_rate(bits: u64) -> String {
    if bits >= 1_000_000 {
        format!("{} Mb/s", decimals(bits as f64 / 1e6, 1))
    } else {
        format!("{} kb/s", (bits as f64 / 1e3).round())
    }
}

fn sample_rate(rate: u32) -> String {
    format!("{} kHz", decimals(f64::from(rate) / 1e3, 1))
}

/// Seconds as `h:mm:ss.s`, or `m:ss.s` under an hour.
fn time(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    let (hours, minutes) = (tenths / 36_000, tenths / 600 % 60);
    let (whole, tenth) = (tenths / 10 % 60, tenths % 10);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{whole:02}.{tenth}")
    } else {
        format!("{minutes}:{whole:02}.{tenth}")
    }
}

/// Seconds as signed milliseconds, such as `+2 ms`.
fn milliseconds(seconds: f64) -> String {
    let ms = (seconds * 1000.0).round();
    // Not "-0 ms".
    let ms = if ms == 0.0 { 0.0 } else { ms };
    format!("{ms:+} ms")
}

/// Signed seconds, such as `+0.25 s`.
fn seconds(seconds: f64, places: usize) -> String {
    let value = if seconds.abs() < 0.5 / 10f64.powi(places as i32) {
        0.0
    } else {
        seconds
    };
    format!("{value:+.places$} s")
}

#[cfg(test)]
mod tests {
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
}
