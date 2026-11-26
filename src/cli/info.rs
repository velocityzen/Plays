//! `plays info`: what files and streams hold, read with FFmpeg in this
//! process, without asking Plays, as text or JSON.

use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Once;

use ffmpeg_next as ffmpeg;
use serde_json::Value;

use super::{Done, Stop, show};
use crate::media::Source;
use crate::media::probe::{Kind, Probe, StreamProbe, VideoProbe};
use crate::media::tracks::{codec_display_name, language_name};

/// Print what each of `paths` holds to `out`, as JSON if `json`, saying
/// why to `err` for those that can't be read. It fails if any can't.
pub fn info(out: &mut dyn Write, err: &mut dyn Write, json: bool, paths: &[OsString]) -> Done {
    let sources: Vec<Source> = paths
        .iter()
        .map(|path| Source::from_arg(PathBuf::from(path)))
        .collect();
    ready();
    let mut probes = Vec::new();
    let mut failed = false;
    for source in &sources {
        match Probe::of(source) {
            Ok(probe) => probes.push(probe),
            Err(why) => {
                writeln!(err, "plays: {why:#}")?;
                failed = true;
            }
        }
    }
    if json {
        let mut values: Vec<Value> = probes
            .iter()
            .map(|probe| serde_json::to_value(probe).map_err(std::io::Error::other))
            .collect::<Result<_, _>>()?;
        match values.len() {
            0 => {}
            1 if sources.len() == 1 => show::pretty(out, &values.remove(0))?,
            _ => show::pretty(out, &Value::Array(values))?,
        }
    } else {
        let texts: Vec<String> = probes.iter().map(text).collect();
        write!(out, "{}", texts.join("\n"))?;
    }
    if failed {
        return Err(Stop::Exit(1));
    }
    Ok(())
}

/// FFmpeg, set up once as the app sets it up, saying nothing of its own:
/// what can't be read is said here.
fn ready() {
    static FFMPEG: Once = Once::new();
    FFMPEG.call_once(|| {
        ffmpeg::init().ok();
        ffmpeg::util::log::set_level(ffmpeg::util::log::Level::Quiet);
        // SAFETY: once, before anything here opens a stream.
        unsafe {
            ffmpeg::ffi::avformat_network_init();
        }
    });
}

/// `probe` as text: what it is, then a line for each thing it says, under
/// headings, the values lined up.
fn text(probe: &Probe) -> String {
    let mut rows: Vec<(&str, String)> = vec![("Format", probe.format.clone())];
    rows.extend(probe.duration.map(|duration| ("Duration", time(duration))));
    rows.extend(probe.size.map(|size| ("Size", bytes(size))));
    rows.extend(probe.bit_rate.map(|rate| ("Bit rate", bit_rate(rate))));
    for (name, value) in &probe.tags {
        rows.push(("Tags", format!("{name}: {value}")));
    }
    let widest = probe
        .streams
        .iter()
        .map(|stream| stream.index.to_string().len())
        .max()
        .unwrap_or(0);
    for stream in &probe.streams {
        rows.push(("Streams", stream_line(stream, widest)));
    }
    for (index, chapter) in probe.chapters.iter().enumerate() {
        let when = format!("{}–{}", time(chapter.start), time(chapter.end));
        rows.push((
            "Chapters",
            format!("{}  {when}  {}", index + 1, chapter.name(index)),
        ));
    }
    for file in &probe.subtitle_files {
        let mut parts = vec![file.path.clone(), codec_display_name(&file.codec)];
        parts.extend(flags(
            &file.language,
            &file.title,
            false,
            file.forced,
            file.hearing_impaired,
        ));
        rows.push(("Beside it", parts.join(" · ")));
    }
    let width = rows
        .iter()
        .map(|(heading, _)| heading.len())
        .max()
        .unwrap_or(0);
    let mut text = format!("{}\n", probe.source);
    let mut under = "";
    for (heading, value) in rows {
        let shown = if heading == under { "" } else { heading };
        under = heading;
        text.push_str(&format!("{shown:width$}  {value}\n"));
    }
    text
}

/// A stream as a line: its number, what it holds, and what it is.
fn stream_line(stream: &StreamProbe, widest: usize) -> String {
    let kind = match stream.kind {
        Kind::Video if stream.cover => "Cover",
        Kind::Video => "Video",
        Kind::Audio => "Audio",
        Kind::Subtitle => "Subtitles",
        Kind::Attachment => "Attachment",
        Kind::Data => "Data",
    };
    let mut codec = match (stream.codec.as_str(), &stream.codec_tag) {
        ("none", Some(tag)) => tag.clone(),
        ("none", None) => "Unknown".to_owned(),
        (codec, _) => codec_display_name(codec),
    };
    // Unless it's only the codec's name again, as DTS's is.
    if let Some(profile) = stream
        .profile
        .as_ref()
        .filter(|profile| !profile.eq_ignore_ascii_case(&codec))
    {
        codec = format!("{codec} {profile}");
    }
    let mut parts = vec![codec];
    if let Some(video) = &stream.video {
        parts.extend(video_parts(video));
    }
    if let Some(audio) = &stream.audio {
        if audio.sample_rate > 0 {
            parts.push(format!(
                "{} kHz",
                number(f64::from(audio.sample_rate) / 1000.0, 3)
            ));
        }
        parts.extend(
            audio
                .layout
                .clone()
                .or_else(|| (audio.channels > 0).then(|| format!("{} channels", audio.channels))),
        );
        parts.extend(audio.sample_format.map(str::to_owned));
        parts.extend(audio.bits.map(|bits| format!("{bits}-bit")));
    }
    if let Some(subtitle) = &stream.subtitle {
        parts.push(if subtitle.text { "text" } else { "pictures" }.to_owned());
    }
    parts.extend(stream.bit_rate.map(bit_rate));
    parts.extend(flags(
        &stream.language,
        &stream.title,
        stream.default,
        stream.forced,
        stream.hearing_impaired,
    ));
    format!(
        "{:>widest$}  {kind:<10}  {}",
        stream.index,
        parts.join(" · ")
    )
}

/// What a video stream's pictures are: their size and shape, how they're
/// turned, their rate, their pixels and colours, and their light.
fn video_parts(video: &VideoProbe) -> Vec<String> {
    let mut parts = vec![format!("{}×{}", video.width, video.height)];
    if let Some(aspect) = &video.pixel_aspect {
        parts.push(format!(
            "{aspect} pixels, shown {}×{}",
            video.display_width, video.display_height
        ));
    }
    match (video.rotation, video.mirrored) {
        (0, false) => {}
        (0, true) => parts.push("mirrored".to_owned()),
        (degrees, false) => parts.push(format!("turned {degrees}°")),
        (degrees, true) => parts.push(format!("mirrored, turned {degrees}°")),
    }
    parts.extend(
        video
            .frame_rate
            .map(|rate| format!("{} fps", number(rate, 3))),
    );
    parts.extend(
        video
            .first_field
            .map(|field| format!("interlaced, {field} field first")),
    );
    parts.extend(video.pixel_format.clone());
    let colours: Vec<&str> = [video.matrix, video.primaries, video.transfer]
        .into_iter()
        .flatten()
        .collect();
    let range = video.range.map(|range| format!("{range} range"));
    match (colours.is_empty(), range) {
        (true, None) => {}
        (true, Some(range)) => parts.push(range),
        (false, None) => parts.push(colours.join("/")),
        (false, Some(range)) => parts.push(format!("{}, {range}", colours.join("/"))),
    }
    parts.extend(video.hdr.map(str::to_owned));
    if let Some(dolby_vision) = &video.dolby_vision {
        let layers = match (dolby_vision.base_layer, dolby_vision.enhancement_layer) {
            (true, true) => " with its enhancement layer",
            (false, true) => " enhancement layer",
            _ => "",
        };
        parts.push(format!("Dolby Vision {}{layers}", dolby_vision.name));
    }
    parts.extend(
        video
            .mastering_peak
            .map(|nits| format!("mastered at {} nits", number(nits, 1))),
    );
    let light: Vec<String> = [("MaxCLL", video.max_cll), ("MaxFALL", video.max_fall)]
        .into_iter()
        .filter_map(|(name, nits)| nits.map(|nits| format!("{name} {nits} nits")))
        .collect();
    if !light.is_empty() {
        parts.push(light.join(", "));
    }
    parts
}

/// A track's language and title, and how it's marked.
fn flags(
    language: &Option<String>,
    title: &Option<String>,
    default: bool,
    forced: bool,
    hearing_impaired: bool,
) -> Vec<String> {
    let language = language.as_deref().map(language_name);
    // A title that only says its language again, as the menu leaves out.
    let title = title.as_ref().filter(|title| {
        language
            .as_ref()
            .is_none_or(|language| !title.eq_ignore_ascii_case(language))
    });
    let mut flags: Vec<String> = language.clone().into_iter().collect();
    flags.extend(title.map(|title| format!("“{title}”")));
    for (on, name) in [
        (default, "Default"),
        (forced, "Forced"),
        (hearing_impaired, "SDH"),
    ] {
        if on {
            flags.push(name.to_owned());
        }
    }
    flags
}

/// `seconds` as hours, minutes and seconds to the millisecond: 0:01:30.500.
fn time(seconds: f64) -> String {
    // Before it, and what isn't a time, is 0:00:00.000.
    let millis = (seconds * 1000.0).round() as u64;
    let (hours, millis) = (millis / 3_600_000, millis % 3_600_000);
    let (minutes, millis) = (millis / 60_000, millis % 60_000);
    let (seconds, millis) = (millis / 1000, millis % 1000);
    format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// A file's size, in bytes, kilobytes, megabytes or gigabytes, of a
/// thousand each, as the Finder counts.
fn bytes(size: u64) -> String {
    let size = size as f64;
    match size {
        _ if size < 1e3 => format!("{size} bytes"),
        _ if size < 1e6 => format!("{} kB", number(size / 1e3, 1)),
        _ if size < 1e9 => format!("{} MB", number(size / 1e6, 1)),
        _ => format!("{} GB", number(size / 1e9, 1)),
    }
}

/// Bits a second, in kilobits or megabits.
fn bit_rate(rate: u64) -> String {
    let rate = rate as f64;
    if rate < 1e3 {
        format!("{rate} b/s")
    } else if rate < 1e6 {
        format!("{} kb/s", number(rate / 1e3, 0))
    } else {
        format!("{} Mb/s", number(rate / 1e6, 1))
    }
}

/// `value` to `places` decimal places at most, without the zeros after
/// its last digit that counts.
fn number(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

#[cfg(test)]
#[path = "tests/info.rs"]
mod tests;
