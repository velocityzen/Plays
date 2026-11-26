use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::*;
use crate::files::test_folder;
use crate::media::audio::Mixer;
use crate::media::decoder::MediaInfo;
use crate::media::still::{self, Still};
use crate::media::subtitles::Placement;
use crate::media::tests::{http, made, options, stream_media, wait_until};
use crate::media::tracks::TrackKind;

/// A live HLS playlist of the stream's second-long segments `first` to
/// `last`, as a server has it at some moment.
fn live(folder: &Path, first: usize, last: usize) {
    let mut text = format!(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:{first}\n"
    );
    for n in first..=last {
        text.push_str(&format!("#EXTINF:1.0,\nsegment{n}.ts\n"));
    }
    fs::write(folder.join("live.m3u8"), text).unwrap();
}

/// A folder of the stream's segments, served, its live playlist listing
/// `first` to `last`; and a player playing it, once it has started.
fn playing_live(test: &str, first: usize, last: usize) -> (PathBuf, Player) {
    let made = stream_media().join("hls");
    let folder = test_folder(test);
    fs::create_dir_all(&folder).unwrap();
    for n in 0..4 {
        let segment = format!("segment{n}.ts");
        fs::copy(made.join(&segment), folder.join(&segment)).unwrap();
    }
    live(&folder, first, last);
    let addr = http::serve(folder.clone(), None);
    let source = Source::parse_url(&format!("http://{addr}/live.m3u8")).unwrap();
    let mut player = Player::open(source, None, options(true, false));
    let started = wait_until(Duration::from_secs(10), || {
        player.tick();
        player.has_started() && player.frame_pts().is_some()
    });
    assert!(started, "{:?}", player.error());
    assert!(player.is_live());
    (folder, player)
}

/// Tick `player` until it has what it waits for; where it shows.
#[track_caller]
fn landed(player: &mut Player) -> f64 {
    let landed = wait_until(Duration::from_secs(10), || {
        player.tick();
        !player.is_buffering() && player.frame_pts().is_some()
    });
    assert!(
        landed,
        "{:?}, buffering {}, at {:?}",
        player.error(),
        player.is_buffering(),
        player.frame_pts()
    );
    player.frame_pts().unwrap()
}

#[test]
fn a_live_stream_paused_plays_on_from_where_it_is_now() {
    let (folder, mut player) = playing_live("live-edge", 0, 1);
    player.pause();
    let paused = player.frame_pts().unwrap();
    assert!(paused < 1.0, "paused in the first segment, at {paused}");

    // While it's paused, the stream goes on.
    live(&folder, 2, 3);
    player.play();
    assert!(
        player.is_buffering(),
        "it waits for where the stream is now"
    );
    let now = landed(&mut player);
    assert!(now > 1.9, "from the third segment, not {now}");
    assert!(player.position() > 1.9, "at {}", player.position());
}

#[test]
fn a_live_stream_seeks_back_within_what_its_server_keeps_and_on_to_live() {
    // Opened three segments from the live edge: the first is before it.
    let (_folder, mut player) = playing_live("live-seeks", 0, 3);
    let opened = player.frame_pts().unwrap();
    assert!(
        opened.abs() < 0.1,
        "its time starts where it opened, not {opened}"
    );
    assert!(player.seeks_back());
    assert!(player.behind_live().unwrap() < LIVE_NEAR, "it plays live");

    player.seek(-0.5);
    let back = landed(&mut player);
    assert!(
        (back + 0.5).abs() < 0.1,
        "half a second before it opened, not {back}"
    );
    assert!(player.behind_live().unwrap() > 0.4);

    // No further back than the server keeps: where it lands, it says.
    player.seek(-100.0);
    let first = landed(&mut player);
    assert!(
        (first + 1.0).abs() < 0.1,
        "the first segment's start, not {first}"
    );
    assert!(
        (player.position() - first).abs() < 0.3,
        "at {}",
        player.position()
    );

    // Past the live edge, it's live again.
    player.seek(100.0);
    assert!(player.is_buffering(), "it opens the stream again");
    let live = landed(&mut player);
    assert!((live - opened).abs() < 0.1, "where it opened, not {live}");
    assert!(player.behind_live().unwrap() < LIVE_NEAR);
}

#[test]
fn a_live_stream_paused_behind_live_plays_on_from_where_it_paused() {
    let (_folder, mut player) = playing_live("live-behind", 0, 3);
    let opened = player.frame_pts().unwrap();
    player.seek(-0.5);
    let back = landed(&mut player);
    player.pause();
    player.play();
    assert!(!player.is_buffering(), "it plays on as it was");
    let on = landed(&mut player);
    assert!(on - back < 0.2, "from {back}, not {on}");
    assert!(player.behind_live().unwrap() > 0.4);

    player.go_live();
    assert!(player.is_buffering(), "and goes live when asked");
    let live = landed(&mut player);
    assert!((live - opened).abs() < 0.1, "where it opened, not {live}");
}

/// Red, on the left of the pictures made here, and blue, on their right.
const RED: [u8; 3] = [0xc0, 0x40, 0x40];
const BLUE: [u8; 3] = [0x40, 0x40, 0xc0];

/// ffmpeg's lavfi input of `seconds` of pictures `size` across, red on
/// their left and blue on their right, at `rate` frames a second.
fn halves(size: &str, rate: &str, seconds: &str) -> String {
    format!(
        "color=c=0xc04040:s={size}:r={rate}:d={seconds},\
         drawbox=x=iw/2:y=0:w=iw/2:h=ih:c=0x4040c0:t=fill"
    )
}

/// A second's tone at `rate` samples a second, as a lavfi input.
fn tone(rate: u32) -> String {
    format!("sine=frequency=440:sample_rate={rate}:duration=1")
}

/// `name`, made of a second of `halves` of `size` at 25 frames a second,
/// with a tone beside it if `sound`, encoded as `how` says.
fn made_as(name: &str, size: &str, sound: bool, how: &str) -> PathBuf {
    let picture = halves(size, "25", "1");
    let tone = tone(48_000);
    let mut args = vec!["-f", "lavfi", "-i", &picture];
    if sound {
        args.extend(["-f", "lavfi", "-i", &tone]);
    }
    args.extend(how.split(' '));
    made::media(&format!("kinds/{name}"), &args)
}

/// What a player showed and heard of a file, played to its end.
struct Played {
    info: Arc<MediaInfo>,
    /// The times of the frames shown, in order.
    shown: Vec<f64>,
    /// Frames of sound heard on the left and on the right.
    heard: [usize; 2],
    /// The first picture shown.
    first: Option<Still>,
}

/// `path` played to its end, its sound taken as fast as it comes.
fn played(path: &Path) -> Played {
    let mixer = Mixer::detached(48_000);
    let source = Source::File(path.to_owned());
    let mut player = Player::open(source, Some(mixer.clone()), options(true, false));
    let (mut shown, mut heard, mut first) = (Vec::new(), [0; 2], None);
    let mut buffer = vec![0.0f32; 512 * 2];
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) && !player.has_played_out() {
        player.tick();
        if let Some(error) = player.error() {
            panic!("{}: {error}", path.display());
        }
        mixer.mix(&mut buffer, 0.0, Instant::now());
        for frame in buffer.as_chunks::<2>().0 {
            for (side, sample) in frame.iter().enumerate() {
                heard[side] += usize::from(sample.abs() > 1e-6);
            }
        }
        if let Some(pts) = player.frame_pts()
            && shown.last() != Some(&pts)
        {
            shown.push(pts);
            if first.is_none() {
                let picture = player.frame().unwrap();
                first = Some(still::shown(&picture, false).unwrap());
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(player.has_played_out(), "{} played on", path.display());
    Played {
        info: player.info().unwrap(),
        shown,
        heard,
        first,
    }
}

/// The colour of `still`, 8 bits a channel, on average from `left` to
/// `right` of its width (as fractions of it), down its middle half.
fn colour(still: &Still, (left, right): (f64, f64)) -> [f64; 3] {
    let (width, height) = (still.width as usize, still.height as usize);
    let columns = (width as f64 * left) as usize..(width as f64 * right) as usize;
    let rows = height / 4..height * 3 / 4;
    let mut sum = [0.0; 3];
    for y in rows.clone() {
        for x in columns.clone() {
            for (sum, sample) in sum.iter_mut().zip(still.pixels[y * width + x]) {
                *sum += f64::from(sample) / 257.0;
            }
        }
    }
    sum.map(|sum| sum / (rows.len() * columns.len()) as f64)
}

/// `played` of `name` showed its pictures as made: `size`, red on their
/// left and blue on their right, frames from the first to the last; and
/// played its sound on both sides, if `sound`.
fn shows_as_made(name: &str, played: &Played, size: (u32, u32), sound: bool) {
    assert_eq!(played.info.video_size, Some(size), "{name}");
    let first = played.first.as_ref().unwrap();
    assert_eq!((first.width, first.height), size, "{name}: shown");
    for (part, made) in [((0.125, 0.375), RED), ((0.625, 0.875), BLUE)] {
        let shown = colour(first, part);
        assert!(
            shown
                .iter()
                .zip(made)
                .all(|(a, b)| (a - f64::from(b)).abs() < 12.0),
            "{name}: {shown:?}, made {made:?}"
        );
    }
    let shown = &played.shown;
    assert!(shown[0] < 0.05, "{name}: first shown at {}", shown[0]);
    assert!(shown.is_sorted(), "{name}: {shown:?}");
    let last = shown.last().unwrap();
    assert!(*last > 0.9, "{name}: last shown at {last}");
    assert_eq!(played.info.has_audio, sound, "{name}");
    if sound {
        let [left, right] = played.heard;
        assert!(
            left > 24_000 && right > 24_000,
            "{name}: heard {left}, {right}"
        );
    }
}

/// Each `kinds` (its name, its size, whether it has sound, and how it's
/// encoded) made, played and shown as made, at once.
fn play_each(kinds: &[(&str, &str, bool, &str)]) {
    std::thread::scope(|scope| {
        for &(name, size, sound, how) in kinds {
            scope.spawn(move || {
                let played = played(&made_as(name, size, sound, how));
                // Shown a pixel smaller where it's odd, to keep it even.
                let even = |side: &str| side.parse::<u32>().unwrap() & !1;
                let (width, height) = size.split_once('x').unwrap();
                shows_as_made(name, &played, (even(width), even(height)), sound);
            });
        }
    });
}

/// Video of each codec plays, in the containers it comes in, its sound
/// with it.
#[test]
fn plays_video_of_each_codec() {
    play_each(&[
        ("vp8.webm", "160x120", true, "-c:v libvpx -c:a libopus"),
        ("vp9.webm", "160x120", true, "-c:v libvpx-vp9 -c:a libopus"),
        ("av1.webm", "160x120", true, "-c:v libsvtav1 -c:a libopus"),
        (
            "hevc.mkv",
            "160x120",
            true,
            "-c:v libx265 -x265-params log-level=error -c:a aac",
        ),
        (
            "hevc.ts",
            "160x120",
            true,
            "-c:v libx265 -x265-params log-level=error -c:a ac3 -ac 6",
        ),
        ("h264.mov", "160x120", true, "-c:v libx264 -c:a aac"),
        (
            "mpeg4.avi",
            "160x120",
            true,
            "-c:v mpeg4 -vtag xvid -c:a libmp3lame",
        ),
        ("mpeg1.mpg", "160x120", true, "-c:v mpeg1video -c:a mp2"),
        (
            "mpeg2.m2ts",
            "160x120",
            true,
            "-c:v mpeg2video -c:a mp2 -mpegts_m2ts_mode 1",
        ),
        (
            "mjpeg.mov",
            "160x120",
            true,
            "-c:v mjpeg -pix_fmt yuvj422p -c:a pcm_s16le",
        ),
        (
            "prores.mov",
            "160x120",
            true,
            "-c:v prores_ks -profile:v 2 -c:a pcm_s24le",
        ),
        (
            "dnxhr.mxf",
            "256x144",
            false,
            // Tagged BT.709, as DNxHR is.
            "-vf scale=in_color_matrix=bt601:out_color_matrix=bt709 \
             -c:v dnxhd -profile:v dnxhr_lb -pix_fmt yuv422p",
        ),
        // Its pictures say their pixels are 12:11, as H.263's are; its
        // container, square.
        ("h263.3gp", "176x144", true, "-c:v h263 -c:a aac -ac 1"),
        (
            "flv1.flv",
            "160x120",
            true,
            "-c:v flv -c:a libmp3lame -ar 44100",
        ),
        ("wmv2.wmv", "160x120", true, "-c:v wmv2 -c:a wmav2"),
    ]);
}

/// Video of each depth and chroma plays in its colours: 10 and 12 bits,
/// 4:2:2 and 4:4:4 (at an odd size), with alpha, and interlaced.
#[test]
fn plays_video_of_each_depth_and_chroma() {
    play_each(&[
        (
            "h264-10.mp4",
            "160x120",
            false,
            "-c:v libx264 -pix_fmt yuv420p10le",
        ),
        (
            "h264-422.mkv",
            "160x120",
            false,
            "-c:v libx264 -pix_fmt yuv422p",
        ),
        (
            "h264-444.mkv",
            "159x119",
            false,
            "-c:v libx264 -pix_fmt yuv444p",
        ),
        (
            "hevc-12.mkv",
            "160x120",
            false,
            "-c:v libx265 -pix_fmt yuv420p12le -x265-params log-level=error",
        ),
        (
            "hevc-422.mkv",
            "160x120",
            false,
            "-c:v libx265 -pix_fmt yuv422p10le -x265-params log-level=error",
        ),
        (
            "vp9-10.webm",
            "160x120",
            false,
            "-c:v libvpx-vp9 -pix_fmt yuv420p10le",
        ),
        (
            "vp9-444.webm",
            "160x120",
            false,
            "-c:v libvpx-vp9 -pix_fmt yuv444p",
        ),
        (
            "av1-10.mkv",
            "160x120",
            false,
            "-c:v libsvtav1 -pix_fmt yuv420p10le",
        ),
        (
            "prores-4444.mov",
            "160x120",
            false,
            "-c:v prores_ks -profile:v 4 -pix_fmt yuva444p10le",
        ),
        (
            "interlaced.ts",
            "160x120",
            true,
            "-c:v libx264 -flags +ildct+ilme -x264opts tff=1 -c:a ac3",
        ),
    ]);
}

/// Pictures show at the shape of their pixels: as the pictures give it
/// or, over that, as their container does; and at 4K, as they are.
#[test]
fn shows_pictures_at_the_shape_of_their_pixels() {
    // 4:3 pixels, as the H.264 stream says, in MPEG-TS, which says nothing
    // of them.
    let wide = made_as("sar.ts", "160x120", false, "-vf setsar=4/3 -c:v libx264");
    // 3:2 ones, as the Matroska file says over them.
    let wider = made::media(
        "kinds/aspect.mkv",
        &["-i", wide.to_str().unwrap(), "-c", "copy", "-aspect", "2:1"],
    );
    // Untagged, as BT.709, which pictures this big are taken to be.
    let big = made_as(
        "4k.mp4",
        "3840x2160",
        false,
        "-vf scale=in_color_matrix=bt601:out_color_matrix=bt709 -c:v libx264 -preset ultrafast",
    );
    for (path, size) in [(wide, (212, 120)), (wider, (240, 120)), (big, (3840, 2160))] {
        let name = path.file_name().unwrap().to_string_lossy();
        shows_as_made(&name, &played(&path), size, false);
    }
}

/// A picture source of `halves` at `rate` frames a second, and then
/// `filters`, for a second.
fn timed(name: &str, rate: &str, filters: &str, how: &str) -> PathBuf {
    let picture = format!("{}{filters}", halves("160x120", rate, "1"));
    let mut args = vec!["-f", "lavfi", "-i", &picture, "-c:v", "libx264"];
    args.extend(how.split(' ').filter(|arg| !arg.is_empty()));
    made::media(&format!("kinds/{name}"), &args)
}

/// Frames show at the times their file gives them: NTSC's 24000 every
/// 1001 seconds, 120 a second, and at a rate that changes.
#[test]
fn shows_frames_at_the_times_their_file_gives() {
    for (name, rate, every) in [
        ("ntsc.mp4", "24000/1001", 1001.0 / 24000.0),
        ("120.mp4", "120", 1.0 / 120.0),
    ] {
        let played = played(&timed(name, rate, "", ""));
        let fps = played.info.frame_rate.unwrap();
        assert!((fps * every - 1.0).abs() < 1e-6, "{name}: {fps} a second");
        for pts in &played.shown {
            let frames = pts / every;
            assert!(
                (frames - frames.round()).abs() < 0.01,
                "{name}: a frame at {pts}"
            );
        }
        let all = (1.0 / every) as usize;
        assert!(played.shown.len() > all / 2, "{name}: {:?}", played.shown);
    }
    // Every frame of the first fifth of a second at 50 a second, then
    // every other.
    let played = played(&timed(
        "vfr.mkv",
        "50",
        ",select='lt(n,10)+not(mod(n,2))'",
        "-fps_mode vfr",
    ));
    for pts in &played.shown {
        let frame = (pts * 50.0).round() as u32;
        assert!(
            (pts * 50.0 - f64::from(frame)).abs() < 0.05,
            "a frame at {pts}"
        );
        assert!(
            frame < 10 || frame.is_multiple_of(2),
            "a frame left out, at {pts}"
        );
    }
    let first = played.shown.iter().filter(|&&pts| pts < 0.19).count();
    assert!(first > 5, "{:?}", played.shown);
}

/// `name`, a second's tone at `rate`, encoded as `how` says.
fn sound_as(name: &str, rate: u32, how: &str) -> PathBuf {
    let tone = tone(rate);
    let mut args = vec!["-f", "lavfi", "-i", &tone];
    args.extend(how.split(' '));
    made::media(&format!("kinds/{name}"), &args)
}

/// `path` played as a sound of `channels` at `rate`, heard on both sides
/// for most of `seconds`.
fn heard_as_made(path: &Path, rate: u32, channels: u16, seconds: f64) {
    let name = path.file_name().unwrap().to_string_lossy();
    let played = played(path);
    let details = played.info.audio_details.clone().unwrap();
    assert_eq!(
        (details.sample_rate, details.channels),
        (rate, channels),
        "{name}"
    );
    let [left, right] = played.heard;
    let most = (seconds * 48_000.0 * 0.75) as usize;
    assert!(left > most && right > most, "{name}: heard {left}, {right}");
}

/// Sound of each codec plays, at its rate and in its container.
#[test]
fn plays_sound_of_each_codec() {
    let kinds: [(&str, u32, u16, &str); 14] = [
        ("mp3.mp3", 44_100, 2, "-ac 2 -c:a libmp3lame"),
        ("mp3-22k.mp3", 22_050, 1, "-c:a libmp3lame"),
        ("vorbis.ogg", 44_100, 2, "-ac 2 -c:a vorbis -strict -2"),
        ("alac.m4a", 44_100, 2, "-ac 2 -c:a alac"),
        ("pcm.aiff", 44_100, 2, "-ac 2 -c:a pcm_s16be"),
        ("s24.wav", 96_000, 1, "-c:a pcm_s24le"),
        ("s32.wav", 192_000, 1, "-c:a pcm_s32le"),
        ("f32.wav", 48_000, 1, "-c:a pcm_f32le"),
        ("u8.wav", 11_025, 1, "-c:a pcm_u8"),
        ("flac.flac", 88_200, 1, "-c:a flac"),
        ("wma.wma", 44_100, 2, "-ac 2 -c:a wmav2"),
        ("wavpack.wv", 44_100, 1, "-c:a wavpack"),
        ("mp2.mp2", 32_000, 1, "-c:a mp2"),
        ("opus.mka", 48_000, 2, "-ac 2 -c:a libopus"),
    ];
    std::thread::scope(|scope| {
        for (name, rate, channels, how) in kinds {
            scope.spawn(move || heard_as_made(&sound_as(name, rate, how), rate, channels, 1.0));
        }
    });
}

/// Surround sound plays mixed down to both sides, its centre on each.
#[test]
fn mixes_surround_sound_down_to_both_sides() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let kinds = [
        // A tone on the centre alone, as ffmpeg spreads one channel.
        (sound_as("aac-5.1.m4a", 48_000, "-ac 6 -c:a aac"), 6, 1.0),
        (sound_as("aac-7.1.m4a", 48_000, "-ac 8 -c:a aac"), 8, 1.0),
        (
            sound_as("opus-5.1.ogg", 48_000, "-ac 6 -c:a libopus"),
            6,
            1.0,
        ),
        (
            sound_as("dts.mka", 48_000, "-ac 6 -c:a dca -strict -2"),
            6,
            1.0,
        ),
        (
            sound_as("truehd.mka", 48_000, "-ac 6 -c:a truehd -strict -2"),
            6,
            1.0,
        ),
        (fixtures.join("clip-sound-ac3.mkv"), 6, 2.0),
        (fixtures.join("clip-sound-eac3.mkv"), 6, 2.0),
    ];
    std::thread::scope(|scope| {
        for (path, channels, seconds) in &kinds {
            scope.spawn(move || heard_as_made(path, 48_000, *channels, *seconds));
        }
    });
}

/// The words of the subtitles showing, run together, and where each goes.
fn words(player: &Player) -> (String, Vec<Placement>) {
    let cues = player.subtitles();
    let texts = cues.iter().flat_map(|cue| &cue.texts);
    let words = texts.clone().map(|text| text.text.as_str()).collect();
    (words, texts.map(|text| text.placement).collect())
}

/// What `path` shows half a second in, its first subtitle track picked.
fn shown_half_way(path: &Path) -> (String, Vec<Placement>) {
    let mut player = Player::open(Source::File(path.to_owned()), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.info().is_some()
    }));
    let info = player.info().unwrap();
    let track = info.tracks(TrackKind::Subtitle).next().unwrap().index;
    player.select_track(TrackKind::Subtitle, Some(track));
    player.seek(0.5);
    wait_until(Duration::from_secs(5), || {
        player.tick();
        !words(&player).0.is_empty()
    });
    words(&player)
}

const SUBRIP: &str = "1\n00:00:00,200 --> 00:00:00,900\nHello there\n";
const WEBVTT: &str = "WEBVTT\n\n00:00.200 --> 00:00.900 line:0\nHello <b>there</b>\n";
const ASS: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 160\nPlayResY: 120\n\n\
    [V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, \
    OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, \
    Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
    Style: Default,Arial,12,&H0000FFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,\
    1,1,0,8,4,4,4,1\n\n\
    [Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
    Dialogue: 0,0:00:00.20,0:00:00.90,Default,,0,0,0,,Hello {\\i1}there\n";

/// Subtitles of each kind show their words in their time, where they go:
/// WebVTT beside the video and in Matroska and WebM, MP4's timed text,
/// and ASS in Matroska.
#[test]
fn shows_subtitles_of_each_kind() {
    let picture = halves("160x120", "25", "1");
    let with = |name: &str, input: (&str, &str), how: &str| {
        let mut args = vec!["-f", "lavfi", "-i", &picture, "-i", input.0];
        args.extend(how.split(' '));
        made::made_from("ffmpeg", &format!("kinds/{name}"), &[input], &args)
    };
    let beside = made::in_own_folder(
        &made_as("clip.mkv", "160x120", false, "-c:v libx264"),
        "kinds-vtt",
        &[("clip.vtt", WEBVTT)],
    );
    let kinds = [
        (beside, Placement::Top),
        (
            with("vtt.mkv", ("s.srt", SUBRIP), "-c:v libx264 -c:s webvtt"),
            Placement::Bottom,
        ),
        (
            with("vtt.webm", ("s.srt", SUBRIP), "-c:v libvpx-vp9 -c:s webvtt"),
            Placement::Bottom,
        ),
        (
            with(
                "timed-text.mp4",
                ("s.srt", SUBRIP),
                "-c:v libx264 -c:s mov_text",
            ),
            Placement::Bottom,
        ),
        (
            with("ass.mkv", ("s.ass", ASS), "-c:v libx264 -c:s copy"),
            Placement::Top,
        ),
    ];
    for (path, placement) in kinds {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            shown_half_way(&path),
            ("Hello there".to_owned(), vec![placement]),
            "{name}"
        );
    }
}

/// DVB's picture subtitles, as broadcast in MPEG-TS, show where they're
/// drawn: those of `clip-dvdsub.mkv`, 80×16 at (40, 96) or (40, 8) of
/// 160×120.
#[test]
fn shows_dvb_subtitles() {
    let dvd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clip-dvdsub.mkv");
    let path = made::media(
        "kinds/dvb.ts",
        &[
            "-i",
            dvd.to_str().unwrap(),
            "-map",
            "0:v",
            "-map",
            "0:s",
            "-c:v",
            "libx264",
            "-c:s",
            "dvbsub",
        ],
    );
    let mut player = Player::open(Source::File(path), None, options(true, false));
    let mut rect = None;
    let shown = wait_until(Duration::from_secs(5), || {
        player.tick();
        if let Some(info) = player.info()
            && info.subtitle_track.is_none()
        {
            let track = info.tracks(TrackKind::Subtitle).next().unwrap().index;
            player.select_track(TrackKind::Subtitle, Some(track));
        }
        let cues = player.subtitles();
        rect = cues
            .iter()
            .flat_map(|cue| &cue.bitmaps)
            .map(|b| b.rect)
            .next();
        rect.is_some()
    });
    assert!(shown, "no picture subtitle: {:?}", player.error());
    let rect = rect.unwrap();
    let at = |top: f32| {
        let placed = [40. / 160., top / 120., 80. / 160., 16. / 120.];
        rect.iter().zip(placed).all(|(a, b)| (a - b).abs() < 1e-3)
    };
    assert!(at(96.) || at(8.), "at {rect:?}");
}

/// The HLS of `stream_media`'s stream in fragmented MP4 segments, in
/// `hls-fmp4/` of the folder it returns.
fn fragmented_hls() -> PathBuf {
    let dir = stream_media();
    let ts = dir.join("stream.ts");
    let hls = [
        "-c",
        "copy",
        "-f",
        "hls",
        "-hls_time",
        "1",
        "-hls_list_size",
        "0",
        "-hls_playlist_type",
        "vod",
        "-hls_segment_type",
        "fmp4",
        // From ADTS, as MPEG-TS has it, to MP4's own.
        "-bsf:a",
        "aac_adtstoasc",
    ];
    made::made_in(&dir, "hls-fmp4", &format!("{hls:?}"), |into| {
        fs::create_dir(into).map_err(|err| err.to_string())?;
        let status = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&ts)
            .args(hls)
            .arg("-hls_segment_filename")
            .arg(into.join("segment%d.m4s"))
            .arg(into.join("vod.m3u8"))
            .status()
            .map_err(|err| err.to_string())?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| "ffmpeg couldn't make the HLS of fragmented MP4".to_owned())
    });
    dir
}

/// HLS of fragmented MP4 segments plays and seeks, as of MPEG-TS ones.
#[test]
fn plays_and_seeks_hls_of_fragmented_mp4() {
    let addr = http::serve(fragmented_hls(), None);
    let source = Source::parse_url(&format!("http://{addr}/hls-fmp4/vod.m3u8")).unwrap();
    let mixer = Some(Mixer::detached(48_000));
    let mut player = Player::open(source, mixer, options(false, false));
    let opened = wait_until(Duration::from_secs(10), || {
        player.tick();
        player.frame_pts().is_some()
    });
    assert!(opened, "{:?}", player.error());
    let info = player.info().unwrap();
    assert!(!info.live, "{info:?}");
    assert!(info.has_audio, "{info:?}");
    assert!((info.duration.unwrap() - 4.0).abs() < 0.2, "{info:?}");
    player.seek(2.5);
    let landed = wait_until(Duration::from_secs(10), || {
        player.tick();
        player
            .frame_pts()
            .is_some_and(|pts| (pts - 2.5).abs() < 0.045)
    });
    assert!(landed, "seek landed on {:?}", player.frame_pts());
}
