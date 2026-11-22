//! The console: what plays in a tile, stream by stream as the file has
//! them, and how playback is going, kept up to date while it shows. `~`
//! shows and hides it on the tile in front.

use gpui::prelude::*;
use gpui::{Context, Div, div};

use super::PlayerView;
use crate::media::decoder::MediaInfo;
use crate::media::details::{AudioDetails, VideoDetails};
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
        let player = self.playback.local();
        let info = player.info();
        let facts = Facts {
            source: player.source().to_string(),
            info: info.as_deref(),
            stats: player.stats(),
            position: player.position(),
            duration: player.duration().filter(|_| !player.is_live()),
            picture: player.frame().map(|frame| describe(&frame)),
            output_rate: player.mixer().map(|mixer| mixer.sample_rate()),
            audio_delay: player.audio_delay(),
            subtitle_delay: player.subtitle_delay(),
            buffering: player.is_buffering(),
            network: player.is_network(),
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
    file.extend(file_rows(facts, info));
    let mut sections = vec![file];
    if let Some(video) = &info.video_details {
        sections.push(video_rows(facts, info, video));
    }
    if let Some(audio) = &info.audio_details {
        sections.push(audio_rows(facts, info, audio));
    }
    sections.extend(subtitle_rows(facts, info));
    sections
}

/// The file's format, where it plays, and how far ahead it is read.
fn file_rows(facts: &Facts, info: &MediaInfo) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut format = info.container.format.clone();
    if let Some(rate) = info.container.bit_rate {
        format = joined([format, bit_rate(rate)]);
    }
    rows.push(("Format", format));
    rows.push((
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
    rows.push((if facts.network { "Downloaded" } else { "Read" }, read));
    rows
}

/// The video's format and color, how it is decoded, and how it shows.
fn video_rows(facts: &Facts, info: &MediaInfo, video: &VideoDetails) -> Vec<Row> {
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
    rows
}

/// The sound's format, what it is played as, and its delay.
fn audio_rows(facts: &Facts, info: &MediaInfo, audio: &AudioDetails) -> Vec<Row> {
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
    rows
}

/// The subtitles showing, and their delay.
fn subtitle_rows(facts: &Facts, info: &MediaInfo) -> Option<Vec<Row>> {
    let subtitles = info
        .subtitle_track
        .and_then(|index| info.tracks.iter().find(|t| t.index == index))
        .filter(|track| track.kind == TrackKind::Subtitle);
    subtitles.map(|track| {
        let number = info
            .tracks(TrackKind::Subtitle)
            .position(|t| t.index == track.index)
            .map_or(1, |at| at + 1);
        vec![
            ("Subtitles", joined([track.label(number), track.detail()])),
            ("Delay", seconds(facts.subtitle_delay, 1)),
        ]
    })
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
#[path = "tests/console.rs"]
mod tests;
