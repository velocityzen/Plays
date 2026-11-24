//! End-to-end checks of the decoding engine against small files generated
//! with the `ffmpeg` command line tool. Without it they fail, rather than
//! pass having checked nothing.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ffmpeg_next::Rational;

use super::audio::Mixer;
use super::decoder::output_size;
use super::external;
use super::orientation::Orientation;
use super::output::{self, AudioOutput};
use super::picking::{Picked, Subtitles, Wanted};
use super::player::{Player, PlayerOptions};
use super::source::Source;
use super::tracks::TrackKind;

mod made;
mod playlists;
use made::{in_own_folder, kept, made_from, made_in, media};

fn options(autoplay: bool, looping: bool) -> PlayerOptions {
    PlayerOptions {
        autoplay,
        looping,
        volume: 1.0,
        max_short_side: None,
        hardware_decoding: false,
        wanted: Wanted::default(),
    }
}

/// A decoder thread failing as a bug would, once the file is open: the block
/// says it can't play it, and stops waiting for more.
#[test]
fn a_decoder_thread_failing_says_so_and_stops_waiting() {
    let dir = crate::files::test_folder("decoder-panics");
    std::fs::create_dir_all(&dir).unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let path = dir.join(format!("{}.mp4", super::decoder::PANICS_IN_DEMUX));
    std::fs::copy(fixtures.join("clip.mp4"), &path).unwrap();
    let mut player = Player::open(Source::File(path), None, options(true, false));
    assert!(
        wait_until(Duration::from_secs(5), || player.error().is_some()),
        "no error said"
    );
    assert!(player.is_known(), "it had opened");
    assert!(player.error().unwrap().contains("as the test asked"));
    assert!(!player.is_buffering());
    assert!(!player.tick().animate, "nothing more to draw");
}

/// A paused player with its queues full lets its decode thread sleep,
/// rather than look for something to do hundreds of times a second.
#[test]
fn a_paused_player_lets_its_decoder_sleep() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(fixtures.join("clip.mp4")),
        Some(mixer.clone()),
        options(false, false),
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    // The picture and sound queues fill, as soon as the machine lets
    // them; then there's nothing to do, and half a second passes with
    // hardly a look. Busy filling them, it looks more.
    let mut looked = Vec::new();
    let quiet = wait_until(Duration::from_secs(10), || {
        let before = player.decoder_waits();
        std::thread::sleep(Duration::from_millis(500));
        looked.push(player.decoder_waits() - before);
        looked.last() <= Some(&5)
    });
    assert!(quiet, "it looked {looked:?} times in each half second");
}

/// What a file holds is shared with whatever asks, every frame, not
/// copied for each; picking a track makes it anew, leaving what was handed
/// out as it was.
#[test]
fn what_a_file_holds_is_shared_not_copied() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut player = Player::open(
        Source::File(fixtures.join("clip-subs.mkv")),
        Some(Mixer::detached(48_000)),
        options(false, false),
    );
    assert!(wait_until(Duration::from_secs(5), || player.is_known()));
    let first = player.info().unwrap();
    assert!(Arc::ptr_eq(&first, &player.info().unwrap()), "copied");
    assert!(player.has_video() && player.has_audio());
    assert_eq!(first.selected(TrackKind::Subtitle), None);

    player.select_track(TrackKind::Subtitle, Some(2));
    let picked = player.info().unwrap();
    assert!(!Arc::ptr_eq(&first, &picked));
    assert_eq!(picked.selected(TrackKind::Subtitle), Some(2));
    assert_eq!(
        first.selected(TrackKind::Subtitle),
        None,
        "what was handed out"
    );
}

fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    false
}

#[test]
fn opens_and_seeks_video_accurately() {
    let path = media(
        "video.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=4",
            "-c:v",
            "libx264",
            "-g",
            "50",
            "-pix_fmt",
            "yuv420p",
        ],
    );
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let info = player.info().unwrap();
    assert!(info.has_video && !info.has_audio);
    assert_eq!(info.video_size, Some((320, 240)));
    assert!(
        (info.duration.unwrap() - 4.0).abs() < 0.1,
        "{:?}",
        info.duration
    );
    assert!(player.frame_pts().unwrap().abs() < 0.01);

    // 2.5 s is mid-GOP, so this has to decode forward from the keyframe at 0.
    for target in [2.5, 0.52, 3.9] {
        player.seek(target);
        assert!(
            wait_until(Duration::from_secs(5), || {
                player.tick();
                player
                    .frame_pts()
                    .is_some_and(|pts| (pts - target).abs() < 0.045)
            }),
            "seek to {target} landed on {:?}",
            player.frame_pts()
        );
    }
}

/// A file whose picture runs `picture` seconds and its sound `sound`, in
/// `container`.
fn ends_apart(picture: u32, sound: u32, container: &str) -> PathBuf {
    media(
        &format!("ends/picture-{picture}-sound-{sound}.{container}"),
        &[
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc=size=160x120:rate=25:duration={picture}"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:sample_rate=48000:duration={sound}"),
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
        ],
    )
}

/// A player of `path` that has opened it, and the mixer it plays to.
fn opened(path: PathBuf) -> (Player, Arc<Mixer>) {
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(true, false),
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.info().is_some()
    }));
    (player, mixer)
}

/// A clip of `seconds` of picture with a tone beside it, `sound` passed
/// to ffmpeg for it: where it starts, or how it's timed.
fn with_sound(name: &str, seconds: u32, sound: &[&str]) -> PathBuf {
    let picture = format!("testsrc=size=160x120:rate=25:duration={seconds}");
    let mut args = vec!["-f", "lavfi", "-i", &picture];
    args.extend_from_slice(sound);
    args.extend_from_slice(&["-c:v", "libx264", "-c:a", "aac"]);
    media(name, &args)
}

/// Play on for `seconds`, as an output would, in real time: the frames
/// shown, and the longest run of silence between the first sound and the
/// last, in frames.
fn play_for(player: &mut Player, mixer: &Mixer, seconds: f64) -> (Vec<f64>, usize) {
    let mut shown = Vec::new();
    let (mut heard, mut silence, mut longest) = (false, 0, 0);
    let mut buffer = vec![0.0f32; 480 * 2];
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < seconds {
        player.tick();
        mixer.mix(&mut buffer, 0.0, Instant::now());
        for frame in buffer.as_chunks::<2>().0 {
            if frame[0] == 0.0 {
                silence += 1;
            } else {
                if heard {
                    longest = longest.max(silence);
                }
                (heard, silence) = (true, 0);
            }
        }
        if let Some(pts) = player.frame_pts()
            && shown.last() != Some(&pts)
        {
            shown.push(pts);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    (shown, longest)
}

/// Whether a frame from `from` to `to` was shown.
fn showed(shown: &[f64], from: f64, to: f64) -> bool {
    shown.iter().any(|pts| (from..to).contains(pts))
}

#[test]
fn sound_starting_after_the_picture_waits_for_it_in_silence() {
    let path = with_sound(
        "with-late-sound.mkv",
        2,
        &[
            "-itsoffset",
            "1",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=1",
        ],
    );
    let (mut player, mixer) = opened(path);
    let (shown, _) = play_for(&mut player, &mixer, 0.8);
    assert!(
        showed(&shown, 0.2, 0.7),
        "the picture plays from the start: {shown:?}"
    );
}

/// A clip whose sound stops for a second, from 0.5 s to 1.5 s, as a
/// recording's can, while its picture plays on: a keyframe every 0.2 s,
/// so a seek into the gap reads no sound till it ends.
fn with_a_sound_gap() -> PathBuf {
    with_sound(
        "with-a-sound-gap-keyed.mkv",
        3,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=2",
            "-af",
            "asetpts='if(gte(T,0.5),PTS+1/TB,PTS)'",
            "-g",
            "5",
        ],
    )
}

#[test]
fn a_gap_in_the_sound_plays_as_silence_and_the_picture_plays_through_it() {
    let (mut player, mixer) = opened(with_a_sound_gap());
    let (shown, longest) = play_for(&mut player, &mixer, 2.5);
    assert!(
        showed(&shown, 0.6, 0.9) && showed(&shown, 1.1, 1.4),
        "the picture plays through it: {shown:?}"
    );
    assert!(
        (40_000..56_000).contains(&longest),
        "a second of silence, not {longest} frames"
    );
}

#[test]
fn a_seek_into_a_gap_in_the_sound_plays_on_from_there() {
    let (mut player, mixer) = opened(with_a_sound_gap());
    player.seek(0.7);
    let (shown, _) = play_for(&mut player, &mixer, 0.7);
    assert!(
        showed(&shown, 0.9, 1.4),
        "the picture plays on from where it went: {shown:?}"
    );
}

#[test]
fn sound_without_gaps_plays_without_any() {
    let path = with_sound(
        "with-sound-whole.mkv",
        1,
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=1",
        ],
    );
    let (mut player, mixer) = opened(path);
    let (_, longest) = play_for(&mut player, &mixer, 0.8);
    assert!(longest < 8, "silent for {longest} frames in it");
}

/// A seek past where one stream ends, into the other's last seconds,
/// plays on to the end rather than waiting for the one that ended.
#[test]
fn plays_on_from_a_seek_past_where_one_stream_ends() {
    for (picture, sound) in [(3, 1), (1, 3)] {
        let (mut player, mixer) = opened(ends_apart(picture, sound, "mkv"));
        player.seek(2.0);
        let heard = drain_audio(&mut player, &mixer, Duration::from_secs(10), |_| false);
        assert!(
            !player.is_playing(),
            "picture {picture} s, sound {sound} s: still buffering: {}",
            player.is_buffering()
        );
        // The sound from 2 s on, where there is any.
        let left = if sound > 2 { 48_000 } else { 0 };
        assert!(
            (heard as i64 - left).abs() < 4_800,
            "picture {picture} s, sound {sound} s: heard {heard} frames"
        );
    }
}

/// A seek into one stream going on long after the other, further than is
/// read ahead, plays, though the file's end, where the other is known to
/// have ended, isn't read by then. An MP4 lands each stream where it's
/// asked to.
#[test]
fn plays_from_a_seek_into_one_stream_going_on_long_after_the_other() {
    for (picture, sound) in [(1, 300), (400, 1)] {
        let (mut player, mixer) = opened(ends_apart(picture, sound, "mp4"));
        player.seek(10.0);
        let [heard, _] =
            drain_each_side(&mut player, &mixer, Duration::from_secs(10), |player, _| {
                player.position() >= 10.5
            });
        assert!(
            player.position() >= 10.5 && player.is_playing() && !player.is_buffering(),
            "picture {picture} s, sound {sound} s: at {} s, buffering: {}",
            player.position(),
            player.is_buffering()
        );
        assert_eq!(
            heard > 0,
            sound > 10,
            "picture {picture} s, sound {sound} s: heard {heard} frames"
        );
    }
}

/// Played again once it has played out, it waits anew for its picture and
/// its sound, rather than taking them as ended still.
#[test]
fn played_again_waits_for_its_picture_and_its_sound() {
    let picture = media(
        "ends/picture-only.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=0.5",
            "-c:v",
            "libx264",
        ],
    );
    let sound = media(
        "ends/sound-only.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=0.25",
        ],
    );
    for path in [picture, sound] {
        let name = path.file_name().unwrap().display().to_string();
        let (mut player, mixer) = opened(path);
        drain_audio(&mut player, &mixer, Duration::from_secs(5), |_| false);
        assert!(player.has_played_out(), "{name}");
        player.play();
        assert!(wait_until(Duration::from_secs(5), || {
            player.tick();
            !player.is_buffering()
        }));
        let info = player.info().unwrap();
        if info.has_video {
            assert!(
                player.frame_pts().is_some_and(|pts| pts.abs() < 0.01),
                "{name}: showing {:?}",
                player.frame_pts()
            );
        }
        if info.has_audio {
            assert!(player.stats().queued_audio > 0.0, "{name}: nothing to play");
        }
    }
}

/// A stream whose sound ends before its picture plays on to the end,
/// neither waiting for more of the sound nor held each time there's none.
#[test]
fn streams_on_past_where_the_sound_ends() {
    let path = ends_apart(2, 1, "mkv");
    let addr = http::serve(path.parent().unwrap().to_path_buf(), None);
    let name = path.file_name().unwrap().to_str().unwrap();
    let source = Source::parse_url(&format!("http://{addr}/{name}")).unwrap();
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(source, Some(mixer.clone()), options(true, false));
    let (mut started, mut held) = (false, 0);
    let [heard, _] = drain_each_side(&mut player, &mixer, Duration::from_secs(15), |player, _| {
        started |= !player.is_buffering();
        held += usize::from(started && player.is_buffering());
        false
    });
    assert!(
        !player.is_playing(),
        "still buffering: {}",
        player.is_buffering()
    );
    assert_eq!(held, 0, "held {held} times once going");
    assert!(
        (heard as i64 - 48_000).abs() < 4_800,
        "heard {heard} frames"
    );
}

/// What a file says of itself once opened, ticking it on till it does.
fn info_of(path: PathBuf) -> Arc<super::decoder::MediaInfo> {
    let mut player = Player::open(
        Source::File(path),
        Some(Mixer::detached(48_000)),
        options(false, false),
    );
    let opened = wait_until(Duration::from_secs(5), || {
        player.tick();
        player.info().is_some()
    });
    assert!(opened);
    player.info().unwrap()
}

const TAGS: [&str; 8] = [
    "-metadata",
    "title=Tone",
    "-metadata",
    "artist=Plays",
    "-metadata",
    "album=Fixtures",
    "-metadata",
    "album_artist=Various Artists",
];

/// A file's tags, and its sound's bits a sample where it keeps them.
#[test]
fn reads_a_songs_tags_and_bits() {
    let mut args = vec![
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=96000:duration=0.3",
        "-c:a",
        "flac",
        "-sample_fmt",
        "s32",
        "-bits_per_raw_sample",
        "24",
    ];
    args.extend(TAGS);
    let info = info_of(media("tags/tone.flac", &args));
    assert_eq!(info.title.as_deref(), Some("Tone"));
    assert_eq!(info.artist.as_deref(), Some("Plays"));
    assert_eq!(info.album.as_deref(), Some("Fixtures"));
    assert_eq!(info.album_artist.as_deref(), Some("Various Artists"));
    let audio = info.audio_details.as_ref().unwrap();
    assert_eq!((audio.bits, audio.sample_rate), (Some(24), 96_000));

    let lossy = info_of(media(
        "tags/tone.m4a",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=0.3",
            "-c:a",
            "aac",
        ],
    ));
    assert_eq!(lossy.audio_details.as_ref().unwrap().bits, None);
    assert_eq!(lossy.title, None);
}

/// PCM keeps no count of its bits but its codec's own.
#[test]
fn reads_the_bits_of_pcm_from_its_codec() {
    let info = info_of(media(
        "tags/tone.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=0.3",
            "-c:a",
            "pcm_s16le",
        ],
    ));
    assert_eq!(info.audio_details.as_ref().unwrap().bits, Some(16));
}

/// Tags lose the spaces around them, and one of spaces alone is none.
#[test]
fn reads_tags_without_the_spaces_around_them() {
    let info = info_of(media(
        "tags/spaced.flac",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=0.3",
            "-c:a",
            "flac",
            "-metadata",
            "title=  Tone ",
            "-metadata",
            "artist=   ",
        ],
    ));
    assert_eq!(
        (info.title.as_deref(), info.artist.as_deref()),
        (Some("Tone"), None)
    );
}

/// Ogg keeps a song's tags on its sound, not on the file.
#[test]
fn reads_the_tags_a_file_with_only_sound_keeps_on_it() {
    let mut args = vec![
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=0.3",
        "-c:a",
        "libopus",
    ];
    args.extend(TAGS);
    let info = info_of(media("tags/tone.opus", &args));
    assert_eq!(info.title.as_deref(), Some("Tone"));
    assert_eq!(info.artist.as_deref(), Some("Plays"));
    assert_eq!(info.album.as_deref(), Some("Fixtures"));
    assert_eq!(info.album_artist.as_deref(), Some("Various Artists"));
}

/// Names written in Windows-1252, not UTF-8, as a WAV's INFO and a
/// Matroska file's often are, read as what they say.
#[test]
fn reads_names_written_in_another_encoding() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let song = info_of(fixtures.join("western-tags.wav"));
    assert_eq!(song.title.as_deref(), Some("Café"));

    let info = info_of(fixtures.join("western-tags.mkv"));
    assert_eq!(info.title.as_deref(), Some("Résumé"));
    let sound: Vec<_> = info
        .tracks(TrackKind::Audio)
        .map(|track| track.title.as_deref())
        .collect();
    assert_eq!(sound, [Some("Français")]);
    let chapters: Vec<_> = info
        .chapters
        .iter()
        .map(|chapter| chapter.title.as_deref())
        .collect();
    assert_eq!(chapters, [Some("Début"), Some("Fin de l'été")]);
}

/// A film's sound may be titled for what it is, such as "Stereo", which
/// isn't the film's title.
#[test]
fn takes_no_tags_from_a_films_sound() {
    let info = info_of(media(
        "tags/film.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=0.4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=0.4",
            "-c:v",
            "libx264",
            "-c:a",
            "libopus",
            "-metadata:s:a:0",
            "title=Stereo",
            "-metadata:s:a:0",
            "artist=Sound Crew",
        ],
    ));
    assert!(info.has_video);
    assert_eq!(
        (info.title.as_deref(), info.artist.as_deref()),
        (None, None)
    );
}

/// A file whose colors FFmpeg has no names for, as a Matroska Colour element
/// can give them (primaries 15, transfer 40 and matrix 30, which the
/// demuxer passes on as they are), plays to the end with them unknown.
#[test]
fn plays_video_whose_colors_ffmpeg_has_no_names_for() {
    ffmpeg_next::init().unwrap();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let source = Source::File(fixtures.join("clip-odd-colors.mkv"));
    let mut player = Player::open(source, None, options(true, false));
    let start = Instant::now();
    let mut last = None;
    while player.is_playing() && start.elapsed() < Duration::from_secs(10) {
        player.tick();
        last = player.frame_pts().or(last);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(player.error().is_none(), "{:?}", player.error());
    assert!(!player.is_playing(), "the player should get to the end");
    assert!(player.frame().is_some());
    assert!(last.is_some_and(|pts| pts > 1.5), "last frame at {last:?}");
    let info = player.info().unwrap();
    let video = info.video_details.as_ref().unwrap();
    assert_eq!(video.pixel_format, Some("yuv420p10le"));
    assert_eq!(
        (video.matrix, video.primaries, video.transfer),
        (None, None, None)
    );
}

/// Pull audio through a mixer until the player stops; returns non-silent frames heard.
fn drain_audio(
    player: &mut Player,
    mixer: &Mixer,
    timeout: Duration,
    stop: impl Fn(usize) -> bool,
) -> usize {
    drain_each_side(player, mixer, timeout, |_, [left, _]| stop(left))[0]
}

/// Pull audio through a mixer until the player stops, or `stop` says to
/// with the player and what's been heard; returns the non-silent frames
/// heard on the left and on the right.
fn drain_each_side(
    player: &mut Player,
    mixer: &Mixer,
    timeout: Duration,
    mut stop: impl FnMut(&Player, [usize; 2]) -> bool,
) -> [usize; 2] {
    let mut heard = [0; 2];
    let mut buffer = vec![0.0f32; 512 * 2];
    let start = Instant::now();
    while start.elapsed() < timeout {
        player.tick();
        if !player.is_playing() || stop(player, heard) {
            break;
        }
        mixer.mix(&mut buffer, 0.0, Instant::now());
        for frame in buffer.as_chunks::<2>().0 {
            for (side, sample) in frame.iter().enumerate() {
                heard[side] += usize::from(sample.abs() > 1e-6);
            }
        }
        std::thread::sleep(Duration::from_micros(300));
    }
    heard
}

#[test]
fn a_player_opened_louder_than_full_plays_at_full() {
    // An eighth of full scale, spread over two channels: 0.088 at its
    // loudest.
    let path = media(
        "volume-tone.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=1",
            "-ac",
            "2",
        ],
    );
    let silent = Player::open(
        Source::File(path.clone()),
        None,
        PlayerOptions {
            volume: -1.0,
            ..options(false, false)
        },
    );
    assert_eq!(silent.volume(), 0.0);
    drop(silent);

    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        PlayerOptions {
            volume: 3.0,
            ..options(true, false)
        },
    );
    assert_eq!(player.volume(), 1.0);

    // A tenth of a second of it, heard.
    let (mut loudest, mut heard) = (0.0f32, 0);
    let mut buffer = vec![0.0f32; 512 * 2];
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) && heard < 4800 * 2 {
        player.tick();
        mixer.mix(&mut buffer, 0.0, Instant::now());
        for sample in &buffer {
            loudest = loudest.max(sample.abs());
            heard += usize::from(*sample != 0.0);
        }
        std::thread::sleep(Duration::from_micros(300));
    }
    assert!(
        (0.08..0.1).contains(&loudest),
        "heard as it is, at {loudest}"
    );
}

#[test]
fn resamples_audio_and_stops_at_the_end() {
    let path = media(
        "tone.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=1",
            "-ac",
            "2",
        ],
    );
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(true, false),
    );
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(10), |_| false);
    assert!(!player.is_playing(), "player should stop at the end");
    // One second of audio, resampled from 44.1 kHz to 48 kHz.
    assert!((heard as i64 - 48_000).abs() < 480, "heard {heard} frames");
    drop(player);
    let _ = Arc::strong_count(&mixer);
}

/// Sound whose channels change partway, as a broadcast's may between a
/// programme and what comes between, plays on through the change.
#[test]
fn plays_on_when_the_sound_changes_its_channels() {
    let half = "sine=frequency=440:sample_rate=48000:duration=0.5";
    let args = |channels| ["-f", "lavfi", "-i", half, "-ac", channels, "-c:a", "aac"];
    let tone = |channels| {
        std::fs::read(media(&format!("channels/{channels}.aac"), &args(channels))).unwrap()
    };
    // ADTS gives the channels in each frame, so the two join as one: mono,
    // then stereo.
    let path = made_in(
        &kept(),
        "channels/mono-then-stereo.aac",
        &format!("{:?} then {:?}", args("1"), args("2")),
        |out| std::fs::write(out, [tone("1"), tone("2")].concat()).map_err(|err| err.to_string()),
    );
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(true, false),
    );
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(10), |_| false);
    assert!(!player.is_playing(), "player should stop at the end");
    // A second, and what the encoder adds before each half.
    assert!(
        (heard as i64 - 48_000).abs() < 4_800,
        "heard {heard} frames"
    );
}

/// Channels a file keeps in an uncommon order, the right before the
/// left, play on their own sides, all of the sound.
#[test]
fn plays_channels_kept_in_an_uncommon_order_on_their_sides() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uncommon-order.mov");
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(true, false),
    );
    let [left, right] = drain_each_side(&mut player, &mixer, Duration::from_secs(10), |_, _| false);
    assert!(!player.is_playing(), "player should stop at the end");
    assert_eq!(left, 0, "the tone is on the right");
    // Half a second, resampled from 8 kHz to 48 kHz.
    assert!((right as i64 - 24_000).abs() < 240, "heard {right} frames");
}

#[test]
fn plays_on_in_time_with_no_device() {
    let path = media(
        "half.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100:duration=0.5",
            "-ac",
            "2",
        ],
    );
    // Plays nowhere until told where: nothing asks it for sound.
    let mixer = output::mixer(1.0);
    let _output = AudioOutput::with_mixer(mixer.clone());
    let mut player = Player::open(Source::File(path), Some(mixer), options(true, false));
    let start = Instant::now();
    while player.is_playing() && start.elapsed() < Duration::from_secs(10) {
        player.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = start.elapsed().as_secs_f64();
    assert!(!player.is_playing(), "the player should get to the end");
    assert!(
        (0.45..3.0).contains(&took),
        "half a second of sound took {took}s"
    );
}

#[test]
fn loops_when_asked() {
    let path = media(
        "short.wav",
        &[
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=300:sample_rate=48000:duration=0.25",
            "-ac",
            "2",
        ],
    );
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(Source::File(path), Some(mixer.clone()), options(true, true));
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(10), |heard| {
        heard > 36_000
    });
    assert!(player.is_playing(), "looping player kept playing");
    assert!(
        heard > 36_000,
        "only heard {heard} frames; the file is 12000 long"
    );
}

/// The words of the subtitles showing now.
fn subtitle_text(player: &Player) -> Vec<String> {
    player
        .subtitles()
        .iter()
        .flat_map(|cue| cue.texts.iter().map(|text| text.text.clone()))
        .collect()
}

/// Seek, and wait until the picture is there and the subtitles say `expected`.
fn subtitles_at(player: &mut Player, time: f64, expected: &[&str]) -> Vec<String> {
    player.seek(time);
    let landed = wait_until(Duration::from_secs(5), || {
        player.tick();
        player
            .frame_pts()
            .is_some_and(|pts| (pts - time).abs() < 0.045)
    });
    assert!(landed, "seek to {time} landed on {:?}", player.frame_pts());
    if expected.is_empty() {
        // Nothing showing is also how it starts, so give a wrong cue time
        // to turn up.
        std::thread::sleep(Duration::from_millis(150));
        player.tick();
    } else {
        wait_until(Duration::from_millis(500), || {
            player.tick();
            subtitle_text(player) == expected
        });
    }
    subtitle_text(player)
}

/// A 4-second video with two audio tracks, English silence and a German
/// tone, and two subtitle tracks, English and French, neither a default.
fn tracks_media() -> PathBuf {
    let first = "1\n00:00:01,000 --> 00:00:02,500\n<i>Hello</i> there\n\n\
                 2\n00:00:03,000 --> 00:00:03,800\nSecond line\n";
    let second = "1\n00:00:01,000 --> 00:00:02,500\nBonjour\n";
    made_from(
        "ffmpeg",
        "tracks.mkv",
        &[("first.srt", first), ("second.srt", second)],
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=4",
            "-f",
            "lavfi",
            "-t",
            "4",
            "-i",
            "anullsrc=r=48000:cl=stereo",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=4",
            "-i",
            "first.srt",
            "-i",
            "second.srt",
            "-map",
            "0",
            "-map",
            "1",
            "-map",
            "2",
            "-map",
            "3",
            "-map",
            "4",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
            // PCM, so the silent track is exactly silent.
            "-c:a",
            "pcm_s16le",
            "-c:s",
            "srt",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "language=ger",
            "-metadata:s:s:0",
            "language=eng",
            "-metadata:s:s:0",
            "title=Full",
            "-metadata:s:s:1",
            "language=fre",
            "-disposition:s:0",
            "0",
            "-disposition:s:1",
            "0",
        ],
    )
}

/// Mix up to `buffers` buffers of 512 frames, until `enough` frames have
/// been heard; returns the non-silent frames heard. Mixing runs faster than
/// real time, so this counts buffers rather than time.
fn mix(player: &mut Player, mixer: &Mixer, buffers: usize, enough: usize) -> usize {
    let mut heard = 0;
    let mut buffer = vec![0.0f32; 512 * 2];
    for _ in 0..buffers {
        player.tick();
        mixer.mix(&mut buffer, 0.0, Instant::now());
        heard += buffer
            .as_chunks::<2>()
            .0
            .iter()
            .filter(|f| f[0].abs() > 1e-6)
            .count();
        if heard >= enough {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    heard
}

#[test]
fn switches_audio_and_subtitle_tracks() {
    let path = tracks_media();
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(false, false),
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));

    let info = player.info().unwrap();
    let audio: Vec<_> = info.tracks(TrackKind::Audio).map(|t| t.index).collect();
    let subtitles: Vec<_> = info.tracks(TrackKind::Subtitle).collect();
    assert_eq!(info.tracks(TrackKind::Video).count(), 1);
    assert_eq!(
        info.tracks(TrackKind::Audio)
            .enumerate()
            .map(|(i, t)| t.label(i + 1))
            .collect::<Vec<_>>(),
        ["English", "German"]
    );
    assert_eq!(subtitles.len(), 2);
    assert_eq!(subtitles[0].label(1), "English – Full");
    assert_eq!(subtitles[0].detail(), "SRT");
    let subtitles: Vec<_> = subtitles.iter().map(|t| t.index).collect();
    assert_eq!(info.audio_track, Some(audio[0]));
    assert_eq!(info.subtitle_track, None, "no subtitle track is a default");

    // The first audio track is silent, the second a tone.
    player.play();
    assert_eq!(
        mix(&mut player, &mixer, 40, usize::MAX),
        0,
        "the first track is silent"
    );
    assert!(player.is_playing() && player.position() < 1.0);
    player.select_track(TrackKind::Audio, Some(audio[1]));
    let heard = mix(&mut player, &mixer, 400, 4800);
    assert!(
        heard >= 4800,
        "heard only {heard} frames of the second track"
    );
    assert_eq!(player.info().unwrap().audio_track, Some(audio[1]));
    player.pause();

    // Picked in the middle of a cue, it shows without a seek, which would
    // drop the frames queued.
    assert_eq!(subtitles_at(&mut player, 1.5, &[]), Vec::<String>::new());
    let serial = player.serial();
    player.select_track(TrackKind::Subtitle, Some(subtitles[0]));
    let shown = wait_until(Duration::from_secs(2), || {
        player.tick();
        subtitle_text(&player) == ["Hello there"]
    });
    assert!(shown, "showing {:?}", subtitle_text(&player));
    assert_eq!(player.serial(), serial, "picking subtitles seeked");
    assert_eq!(
        subtitles_at(&mut player, 1.5, &["Hello there"]),
        ["Hello there"]
    );
    let cues = player.subtitles();
    let spans = &cues[0].texts[0].spans;
    assert!(spans[0].italic && spans[0].len == "Hello".len() && !spans[1].italic);
    assert_eq!(subtitles_at(&mut player, 2.7, &[]), Vec::<String>::new());
    assert_eq!(
        subtitles_at(&mut player, 3.2, &["Second line"]),
        ["Second line"]
    );

    player.select_track(TrackKind::Subtitle, Some(subtitles[1]));
    assert_eq!(subtitles_at(&mut player, 1.2, &["Bonjour"]), ["Bonjour"]);
    assert_eq!(player.info().unwrap().subtitle_track, Some(subtitles[1]));

    player.select_track(TrackKind::Subtitle, None);
    assert!(subtitle_text(&player).is_empty());
    assert_eq!(subtitles_at(&mut player, 1.5, &[]), Vec::<String>::new());
    assert_eq!(player.info().unwrap().subtitle_track, None);
}

/// `tracks_media` opened wanting `wanted`, once it's known: the audio and
/// subtitle tracks it plays.
/// A picture subtitle is placed on the size its track gives or, where it
/// gives none, as in a VOB, on the video's: in fractions of the picture.
#[test]
fn picture_subtitles_are_placed_on_the_size_they_are_drawn_for() {
    let sized = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clip-dvdsub.mkv");
    let remux = "-map 0:v -map 0:s -c:v mpeg2video -c:s copy -f vob";
    let args: Vec<&str> = ["-i", sized.to_str().unwrap()]
        .into_iter()
        .chain(remux.split(' '))
        .collect();
    let sizeless = media("clip-dvdsub.vob", &args);
    // Its cues: 80×16 at (40, 96), then at (40, 8), of 160×120.
    let placed = |top: f32| [40. / 160., top / 120., 80. / 160., 16. / 120.];
    for path in [sized, sizeless] {
        let (mut player, _mixer) = opened(path.clone());
        let info = player.info().unwrap();
        let track = info.tracks(TrackKind::Subtitle).next().unwrap().index;
        player.select_track(TrackKind::Subtitle, Some(track));
        player.seek(0.5);
        let mut rect = None;
        let shown = wait_until(Duration::from_secs(5), || {
            player.tick();
            let cues = player.subtitles();
            rect = cues
                .iter()
                .flat_map(|cue| &cue.bitmaps)
                .map(|b| b.rect)
                .next();
            rect.is_some()
        });
        assert!(shown, "{}: no picture subtitle", path.display());
        let rect = rect.unwrap();
        let at = |placed: [f32; 4]| rect.iter().zip(placed).all(|(a, b)| (a - b).abs() < 1e-3);
        assert!(
            at(placed(96.)) || at(placed(8.)),
            "{}: at {rect:?}",
            path.display()
        );
    }
}

fn opened_wanting(wanted: Wanted, mixer: &Arc<Mixer>) -> (Player, Option<usize>, Option<usize>) {
    let mut player = Player::open(
        Source::File(tracks_media()),
        Some(mixer.clone()),
        PlayerOptions {
            wanted,
            ..options(false, false)
        },
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let info = player.info().unwrap();
    let (audio, subtitles) = (info.audio_track, info.subtitle_track);
    (player, audio, subtitles)
}

#[test]
fn opens_with_the_tracks_wanted() {
    // Video 0, English audio 1 and German 2, English subtitles 3 and
    // French 4.
    let mixer = Mixer::detached(48_000);
    let languages = |audio: &[&str], subtitles: &[&str]| Wanted {
        picked: Picked::default(),
        audio_languages: audio.iter().map(|&l| l.to_owned()).collect(),
        subtitle_languages: subtitles.iter().map(|&l| l.to_owned()).collect(),
    };
    let (_, audio, subtitles) = opened_wanting(Wanted::default(), &mixer);
    assert_eq!((audio, subtitles), (Some(1), None), "as the file has it");

    let (mut player, audio, subtitles) =
        opened_wanting(languages(&["ita", "ger"], &["fre"]), &mixer);
    assert_eq!((audio, subtitles), (Some(2), Some(4)), "German and French");
    player.play();
    let heard = mix(&mut player, &mixer, 400, 4800);
    assert!(heard >= 4800, "the German tone: heard only {heard} frames");
    player.pause();
    assert_eq!(subtitles_at(&mut player, 1.2, &["Bonjour"]), ["Bonjour"]);
    drop(player);

    let mut picked = languages(&["ger"], &["fre"]);
    picked.picked = Picked {
        audio: Some(1),
        subtitles: Some(Subtitles::Track(3)),
    };
    let (_, audio, subtitles) = opened_wanting(picked.clone(), &mixer);
    assert_eq!((audio, subtitles), (Some(1), Some(3)), "as picked before");
    picked.picked.subtitles = Some(Subtitles::Off);
    let (_, audio, subtitles) = opened_wanting(picked, &mixer);
    assert_eq!((audio, subtitles), (Some(1), None), "hidden before");
}

/// Seek or skip, and wait until the picture and the playhead are at `expected`.
fn lands_at(player: &mut Player, go: impl FnOnce(&mut Player), expected: f64) -> (f64, f64) {
    go(player);
    wait_until(Duration::from_secs(5), || {
        player.tick();
        !player.is_buffering()
            && player
                .frame_pts()
                .is_some_and(|pts| (pts - expected).abs() < 0.045)
    });
    (player.frame_pts().unwrap_or(f64::NAN), player.position())
}

/// Skips land on the time asked for when a keyframe is near before it, and
/// otherwise on the nearest keyframe their way, which shows at once.
#[test]
fn skips_land_on_keyframes_far_from_the_target() {
    for container in ["mkv", "mp4"] {
        let path = media(
            &format!("keyframes.{container}"),
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=160x120:rate=25:duration=12",
                "-c:v",
                "libx264",
                "-x264-params",
                "keyint=100:min-keyint=100:scenecut=0",
                "-pix_fmt",
                "yuv420p",
            ],
        );
        let mut player = Player::open(Source::File(path), None, options(false, false));
        assert!(wait_until(Duration::from_secs(5), || {
            player.tick();
            player.frame_pts().is_some()
        }));
        let near = |(pts, position): (f64, f64), time: f64| {
            (pts - time).abs() < 0.045 && (position - time).abs() < 0.045
        };

        // Keyframes are at 0, 4 and 8 seconds.
        let landed = lands_at(&mut player, |p| p.seek(1.0), 1.0);
        assert!(
            near(landed, 1.0),
            "{container}: seek to 1 landed at {landed:?}"
        );
        let landed = lands_at(&mut player, |p| p.seek_by(2.0), 4.0);
        assert!(near(landed, 4.0), "{container}: 1+2 landed at {landed:?}");
        let landed = lands_at(&mut player, |p| p.seek_by(0.5), 4.5);
        assert!(near(landed, 4.5), "{container}: 4+0.5 landed at {landed:?}");
        let landed = lands_at(&mut player, |p| p.seek(11.0), 11.0);
        assert!(
            near(landed, 11.0),
            "{container}: seek to 11 landed at {landed:?}"
        );
        let landed = lands_at(&mut player, |p| p.seek_by(-1.5), 8.0);
        assert!(
            near(landed, 8.0),
            "{container}: 11-1.5 landed at {landed:?}"
        );
        let landed = lands_at(&mut player, |p| p.seek_by(-3.5), 4.5);
        assert!(near(landed, 4.5), "{container}: 8-3.5 landed at {landed:?}");
        // A seek is exact however far the keyframe.
        let landed = lands_at(&mut player, |p| p.seek(7.0), 7.0);
        assert!(
            near(landed, 7.0),
            "{container}: seek to 7 landed at {landed:?}"
        );
    }
}

/// Dragging the slider: a scrub queues the first frame it decodes on its
/// way (a keyframe), then the one asked for, and nothing after it, with no
/// sound; one a little ahead decodes on rather than going back to a
/// keyframe. Letting go plays on as before.
#[test]
fn scrubbing_shows_frames_on_the_way_and_nothing_past_them() {
    let path = media(
        "scrub.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=12",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=12",
            "-c:v",
            "libx264",
            "-x264-params",
            "keyint=100:min-keyint=100:scenecut=0",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ],
    );
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(Source::File(path), Some(mixer), options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some() && !player.is_buffering()
    }));
    assert!(player.stats().queued_audio > 0.0, "sound is decoded ahead");
    let near = |time: f64, expected: f64| (time - expected).abs() < 0.045;
    // Not ticked meanwhile, so all it queued is there to see.
    let scrub = |player: &mut Player, time: f64| {
        player.scrub(time);
        assert!(
            wait_until(Duration::from_secs(5), || player
                .queued()
                .last()
                .is_some_and(|&last| near(last, time))),
            "a scrub to {time} queued {:?}",
            player.queued()
        );
        // Nothing more comes.
        std::thread::sleep(Duration::from_millis(100));
        player.queued()
    };

    // Keyframes are at 0, 4 and 8 seconds.
    // Of the frames between, a few at most.
    let queued = scrub(&mut player, 7.0);
    assert!(
        (2..=10).contains(&queued.len()) && near(queued[0], 4.0),
        "{queued:?}"
    );
    assert!(queued.iter().rev().skip(1).all(|&time| time < 7.0));
    player.tick();
    assert!(player.frame_pts().is_some_and(|pts| near(pts, 7.0)));
    assert!(player.queued().is_empty(), "the latest shows");
    assert!(!player.is_buffering());
    assert_eq!(player.stats().queued_audio, 0.0, "a scrub is silent");

    // Past the keyframe at 8, but near: decoded on from 7.
    let queued = scrub(&mut player, 8.6);
    assert!(queued[0] > 7.0 && queued[0] < 8.0, "{queued:?}");
    player.tick();
    // Back: from the keyframe before.
    let queued = scrub(&mut player, 5.0);
    assert!(near(queued[0], 4.0), "{queued:?}");
    player.tick();
    assert!(player.frame_pts().is_some_and(|pts| near(pts, 5.0)));
    // Far ahead: from the keyframe before.
    let queued = scrub(&mut player, 11.5);
    assert!(near(queued[0], 8.0), "{queued:?}");
    player.tick();

    // Let go: it reads ahead again, sound and all.
    let landed = lands_at(&mut player, |p| p.seek(5.0), 5.0);
    assert!(near(landed.0, 5.0) && near(landed.1, 5.0), "{landed:?}");
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.stats().queued_audio > 0.0 && !player.queued().is_empty()
    }));
}

/// A slow drag forward: scrubs a little ahead of the last, over and over,
/// through a clip whose sound comes in many small packets, 750 a second.
/// The sound isn't played while scrubbing, and what's queued of it, kept,
/// would fill what can be queued in 11 s and stop the picture being read:
/// it follows the slider all the way.
#[test]
fn a_slow_scrub_forward_follows_the_slider_all_the_way() {
    let path = media(
        "with-small-sound-packets.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=20",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:samples_per_frame=64:duration=20",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "pcm_s16le",
        ],
    );
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(Source::File(path), Some(mixer), options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some() && !player.is_buffering()
    }));
    for step in 1..=36 {
        let time = f64::from(step) * 0.5;
        player.scrub(time);
        assert!(
            wait_until(Duration::from_secs(5), || {
                player.tick();
                player
                    .frame_pts()
                    .is_some_and(|pts| (pts - time).abs() < 0.045)
            }),
            "a scrub to {time} shows {:?}",
            player.frame_pts()
        );
    }
}

/// Going back keeps the subtitles on screen, even one that started before
/// the keyframe the seek reads from, and one read again shows once.
#[test]
fn going_back_keeps_the_subtitles() {
    let srt = "1\n00:00:01,000 --> 00:00:02,000\nBefore\n\n\
               2\n00:00:03,500 --> 00:00:05,000\nAcross\n";
    let path = made_from(
        "ffmpeg",
        "across.mkv",
        &[("across.srt", srt)],
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=8",
            "-i",
            "across.srt",
            "-map",
            "0",
            "-map",
            "1",
            // Keyframes at 0 and 4 seconds.
            "-c:v",
            "libx264",
            "-x264-params",
            "keyint=100:min-keyint=100:scenecut=0",
            "-pix_fmt",
            "yuv420p",
            "-c:s",
            "srt",
        ],
    );
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let track = player
        .info()
        .unwrap()
        .tracks(TrackKind::Subtitle)
        .next()
        .unwrap()
        .index;
    player.select_track(TrackKind::Subtitle, Some(track));

    assert_eq!(subtitles_at(&mut player, 3.8, &["Across"]), ["Across"]);
    // From the keyframe at 4, the cue isn't read again, but it's kept.
    assert_eq!(subtitles_at(&mut player, 4.6, &["Across"]), ["Across"]);
    // A frame back, it stays on screen while the seek goes on.
    player.seek(4.56);
    assert_eq!(subtitle_text(&player), ["Across"], "blinked");
    assert_eq!(subtitles_at(&mut player, 4.56, &["Across"]), ["Across"]);
    // Read again from the keyframe at 0, each shows once.
    assert_eq!(subtitles_at(&mut player, 3.7, &["Across"]), ["Across"]);
    assert_eq!(subtitles_at(&mut player, 1.5, &["Before"]), ["Before"]);
    assert_eq!(subtitles_at(&mut player, 2.5, &[]), Vec::<String>::new());
}

#[test]
fn plays_subtitle_files_beside_the_video() {
    let english = "1\n00:00:01,000 --> 00:00:02,500\nHello from a file\n\n\
                   2\n00:00:03,000 --> 00:00:03,800\nSecond line\n";
    // Legacy encodings are common in subtitle files.
    let (russian, _, _) = encoding_rs::WINDOWS_1251
        .encode("1\n00:00:01,000 --> 00:00:02,500\nПривет из файла, как дела?\n");
    let clip = media(
        "beside/clip.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=4",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
        ],
    );
    let files = [
        ("clip.en.srt", english.as_bytes()),
        ("clip.ru.srt", &russian),
    ];
    let path = in_own_folder(&clip, "beside", &files);
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));

    let info = player.info().unwrap();
    let subtitles: Vec<_> = info.tracks(TrackKind::Subtitle).collect();
    let labels: Vec<_> = subtitles
        .iter()
        .enumerate()
        .map(|(i, t)| (t.label(i + 1), t.detail()))
        .collect();
    assert_eq!(
        labels,
        [
            ("English".to_owned(), "SRT · External".to_owned()),
            ("Russian".to_owned(), "SRT · External".to_owned()),
        ]
    );
    let subtitles: Vec<_> = subtitles.iter().map(|t| t.index).collect();
    assert_eq!(
        info.subtitle_track,
        Some(subtitles[0]),
        "a file beside the video shows from the start"
    );

    assert_eq!(
        subtitles_at(&mut player, 3.2, &["Second line"]),
        ["Second line"]
    );
    // Back, and into a cue that started before the seek.
    assert_eq!(
        subtitles_at(&mut player, 2.0, &["Hello from a file"]),
        ["Hello from a file"]
    );
    assert_eq!(subtitles_at(&mut player, 2.7, &[]), Vec::<String>::new());

    player.select_track(TrackKind::Subtitle, Some(subtitles[1]));
    assert_eq!(
        subtitles_at(&mut player, 1.5, &["Привет из файла, как дела?"]),
        ["Привет из файла, как дела?"]
    );
    assert_eq!(player.info().unwrap().subtitle_track, Some(subtitles[1]));
}

/// A 10-second video, `clip.mkv`, with `files` beside it (name and what it
/// says), in a folder of the test's own, `dir`.
fn video_with_subtitle_files(dir: &str, files: &[(&str, &str)]) -> PathBuf {
    let clip = media(
        &format!("{dir}/clip.mkv"),
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=10",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
        ],
    );
    let files: Vec<_> = files
        .iter()
        .map(|(name, text)| (*name, format!("1\n00:00:00,500 --> 00:00:09,500\n{text}\n")))
        .collect();
    in_own_folder(&clip, dir, &files)
}

#[test]
fn keeps_playing_while_a_subtitle_file_is_read() {
    let path = video_with_subtitle_files(
        "slow-to-read",
        &[("clip.en.srt", "Hello"), ("clip.fr.srt", "Bonjour")],
    );
    let french = path.with_file_name("clip.fr.srt");
    external::slow::TO_READ
        .lock()
        .unwrap()
        .push((french, Duration::from_secs(3)));
    let mut player = Player::open(Source::File(path), None, options(true, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        subtitle_text(&player) == ["Hello"]
    }));
    let subtitles: Vec<_> = player
        .info()
        .unwrap()
        .tracks(TrackKind::Subtitle)
        .map(|track| track.index)
        .collect();

    player.select_track(TrackKind::Subtitle, Some(subtitles[1]));
    let from = player.frame_pts().unwrap();
    let clock_from = player.position();
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(1500) {
        player.tick();
        std::thread::sleep(Duration::from_millis(5));
    }
    // The picture kept up with the clock, however fast the machine let
    // the clock go.
    let played = player.frame_pts().unwrap() - from;
    let clocked = player.position() - clock_from;
    assert!(
        clocked > 1.0 && played > clocked - 0.4,
        "played {played:.2}s of {clocked:.2} while it was read"
    );
    assert_eq!(player.info().unwrap().subtitle_track, Some(subtitles[1]));

    assert!(
        wait_until(Duration::from_secs(5), || {
            player.tick();
            subtitle_text(&player) == ["Bonjour"]
        }),
        "showing {:?} once it was read",
        subtitle_text(&player)
    );
}

/// Subtitles picked that can't play, a file gone since it was found or
/// a stream the file doesn't have, leave those that played, and the
/// tracks say so once they've been tried.
#[test]
fn subtitles_that_cant_play_leave_those_that_played() {
    let path = video_with_subtitle_files(
        "gone-before-read",
        &[("clip.en.srt", "Hello"), ("clip.fr.srt", "Bonjour")],
    );
    let mut player = Player::open(Source::File(path.clone()), None, options(true, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        subtitle_text(&player) == ["Hello"]
    }));
    let subtitles: Vec<_> = player
        .info()
        .unwrap()
        .tracks(TrackKind::Subtitle)
        .map(|track| track.index)
        .collect();
    let shown = |player: &Player| player.info().unwrap().subtitle_track;

    std::fs::remove_file(path.with_file_name("clip.fr.srt")).unwrap();
    player.select_track(TrackKind::Subtitle, Some(subtitles[1]));
    assert_eq!(shown(&player), Some(subtitles[1]), "at once");
    assert!(
        wait_until(Duration::from_secs(5), || {
            player.tick();
            shown(&player) == Some(subtitles[0])
        }),
        "put back once it couldn't be read"
    );

    player.select_track(TrackKind::Subtitle, Some(99));
    assert!(
        wait_until(Duration::from_secs(5), || {
            player.tick();
            shown(&player) == Some(subtitles[0])
        }),
        "put back"
    );
    assert_eq!(subtitle_text(&player), ["Hello"]);
}

#[test]
fn opens_before_its_subtitle_files_are_read() {
    let files = [
        ("clip.en.srt", "Hello"),
        ("clip.fr.srt", "Bonjour"),
        ("clip.it.srt", "Ciao"),
    ];
    let path = video_with_subtitle_files("slow-to-open", &files);
    // Each takes a second to open, as on a share far away, and the one
    // shown three to read.
    for (name, _) in files {
        let file = path.with_file_name(name);
        let took = Duration::from_secs(1);
        external::slow::TO_OPEN.lock().unwrap().push((file, took));
    }
    let english = path.with_file_name("clip.en.srt");
    let took = Duration::from_secs(3);
    external::slow::TO_READ
        .lock()
        .unwrap()
        .push((english, took));

    let start = Instant::now();
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(10), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let took = start.elapsed();
    assert!(
        took < Duration::from_secs(2),
        "the first frame took {took:?}"
    );
    let info = player.info().unwrap();
    let subtitles: Vec<_> = info.tracks(TrackKind::Subtitle).collect();
    assert_eq!(subtitles.len(), 3);
    // English, the first by name.
    assert_eq!(
        info.subtitle_track,
        Some(subtitles[0].index),
        "shown from the start"
    );

    player.seek(1.0);
    assert!(
        wait_until(Duration::from_secs(5), || {
            player.tick();
            subtitle_text(&player) == ["Hello"]
        }),
        "showing {:?} once it was read",
        subtitle_text(&player)
    );
}

/// A second of sound, `dir/tone.m4a` in `plays-tests`, with a picture of
/// `own` size in its tags, if any, as cover art.
fn sound_in(dir: &str, own: Option<&str>) -> PathBuf {
    let tone = "sine=frequency=440:sample_rate=48000:duration=1";
    match own {
        None => media(
            &format!("{dir}/tone.m4a"),
            &["-f", "lavfi", "-i", tone, "-c:a", "aac"],
        ),
        Some(size) => media(
            &format!("{dir}/tone.m4a"),
            &[
                "-f",
                "lavfi",
                "-i",
                tone,
                "-f",
                "lavfi",
                "-i",
                &format!("color=c=blue:size={size}"),
                "-frames:v",
                "1",
                "-map",
                "0:a",
                "-map",
                "1:v",
                "-c:a",
                "aac",
                "-c:v",
                "png",
                "-disposition:v:0",
                "attached_pic",
            ],
        ),
    }
}

/// A picture of `size`, `dir/name` in `plays-tests`, in the format its
/// extension says.
fn picture_in(dir: &str, name: &str, size: &str) -> PathBuf {
    media(
        &format!("{dir}/{name}"),
        &[
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=red:size={size}"),
            "-frames:v",
            "1",
        ],
    )
}

/// The size of the cover `player` shows, once it shows one.
fn cover_size(player: &mut Player) -> Option<(i32, i32)> {
    wait_until(Duration::from_secs(5), || {
        player.tick();
        player.cover().is_some()
    });
    player.cover().map(|cover| {
        let size = cover.size(0);
        (size.width.0, size.height.0)
    })
}

#[test]
fn sound_with_no_cover_of_its_own_shows_the_one_beside_it() {
    for (dir, name) in [
        ("cover-png", "cover.png"),
        ("cover-jpg", "Cover.JPG"),
        ("cover-tiff", "cover.tiff"),
    ] {
        let path = sound_in(dir, None);
        picture_in(dir, name, "64x48");
        let mut player = Player::open(
            Source::File(path),
            Some(Mixer::detached(48_000)),
            options(false, false),
        );
        assert_eq!(cover_size(&mut player), Some((64, 48)), "{name}");
    }

    // Scaled down as video is.
    let path = sound_in("cover-png", None);
    let options = PlayerOptions {
        max_short_side: Some(24),
        ..options(false, false)
    };
    let mut player = Player::open(Source::File(path), Some(Mixer::detached(48_000)), options);
    assert_eq!(cover_size(&mut player), Some((32, 24)));
}

#[test]
fn sound_with_a_cover_of_its_own_shows_that_one() {
    // Large, so slower to decode than the one beside it, which would show
    // first were it looked for.
    let path = sound_in("cover-own-large", Some("2400x1600"));
    picture_in("cover-own-large", "cover.png", "64x48");
    let mut player = Player::open(
        Source::File(path),
        Some(Mixer::detached(48_000)),
        options(false, false),
    );
    assert_eq!(cover_size(&mut player), Some((2400, 1600)));
}

#[test]
fn video_shows_no_cover_beside_it() {
    let path = media(
        "cover-video/clip.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ],
    );
    picture_in("cover-video", "cover.png", "64x48");
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    // Longer than sound takes to show the cover beside it.
    assert!(!wait_until(Duration::from_millis(500), || {
        player.tick();
        player.cover().is_some()
    }));
}

#[test]
fn reports_unplayable_files() {
    let path = std::env::temp_dir().join(format!("plays-not-media-{}.txt", std::process::id()));
    std::fs::write(&path, "definitely not a video").unwrap();
    ffmpeg_next::init().unwrap();
    let mut player = Player::open(Source::File(path.clone()), None, options(true, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.error().is_some()
    }));
    std::fs::remove_file(path).ok();
}

#[test]
fn sizes_frames_for_display() {
    let square = Rational::new(1, 1);
    assert_eq!(output_size(1920, 1080, square, None), (1920, 1080));
    assert_eq!(output_size(3840, 2160, square, Some(1080)), (1920, 1080));
    // Portrait video is capped on its short side too.
    assert_eq!(output_size(1080, 1920, square, Some(720)), (720, 1280));
    // Anamorphic DVD: 720x480 with 32:27 pixels is 853x480 on screen (kept even).
    assert_eq!(
        output_size(720, 480, Rational::new(32, 27), None),
        (852, 480)
    );
    // Unknown aspect ratio is treated as square.
    assert_eq!(output_size(641, 361, Rational::new(0, 1), None), (640, 360));
}

/// A file that says to turn its pictures (as phones do) reports which way up
/// they go, and is sized as decoded.
#[test]
fn reports_the_streams_orientation() {
    let upright = media(
        "orientation-source.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=1",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ],
    );
    let upright = upright.to_str().unwrap();
    // A quarter turn counterclockwise, then mirrored, with the pictures as
    // they were.
    let path = media(
        "orientation.mp4",
        &[
            "-display_rotation",
            "90",
            "-display_hflip",
            "-i",
            upright,
            "-c",
            "copy",
        ],
    );
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let info = player.info().unwrap();
    assert_eq!(info.video_size, Some((320, 240)));
    assert_eq!(
        info.orientation,
        Orientation::new(-1, false).flipped_horizontally()
    );
}

/// Delaying the sound moves the picture, and the playhead, ahead of it:
/// the sound plays on as it was.
#[test]
fn delays_the_sound_against_the_picture() {
    let path = media(
        "sync.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=4",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "pcm_s16le",
            "-ac",
            "2",
        ],
    );
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(false, false),
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    player.set_audio_delay(2.0);
    assert_eq!(player.audio_delay(), 2.0);
    player.play();
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(5), |heard| {
        heard >= 24_000
    });
    let heard = heard as f64 / 48_000.0;
    let ahead = player.position() - heard;
    assert!(
        (ahead - 2.0).abs() < 0.1,
        "the playhead is {ahead:.3} s ahead of the sound"
    );
    assert!(
        wait_until(Duration::from_secs(5), || {
            player.tick();
            player.frame_pts().is_some_and(|pts| pts > heard + 1.8)
        }),
        "the picture is at {:?} after {heard:.2} s of sound",
        player.frame_pts()
    );
    player.set_audio_delay(-60.0);
    assert_eq!(player.audio_delay(), -super::player::MAX_AUDIO_DELAY);
}

/// A subtitle delay moves when cues show, and seeking keeps it: a seek
/// lands on a cue that is over by its own time but not as delayed.
#[test]
fn delays_subtitles_against_the_picture() {
    let clip = media(
        "delay/clip.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=4",
            "-c:v",
            "libx264",
            "-g",
            "25",
            "-pix_fmt",
            "yuv420p",
        ],
    );
    let srt = "1\n00:00:01,000 --> 00:00:02,000\nRight on time\n";
    let path = in_own_folder(&clip, "delay", &[("clip.srt", srt)]);
    let mut player = Player::open(Source::File(path), None, options(false, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let cue = ["Right on time"];
    let none = Vec::<String>::new();
    assert_eq!(subtitles_at(&mut player, 1.5, &cue), cue);

    player.set_subtitle_delay(1.0);
    assert_eq!(subtitles_at(&mut player, 1.5, &[]), none);
    assert_eq!(subtitles_at(&mut player, 2.8, &cue), cue);
    assert_eq!(subtitles_at(&mut player, 3.1, &[]), none);

    player.set_subtitle_delay(-0.8);
    assert_eq!(subtitles_at(&mut player, 0.5, &cue), cue);
    assert_eq!(subtitles_at(&mut player, 1.5, &[]), none);
}

/// With the window hidden, frames are only consumed a few times a second,
/// and more are due between than the queue holds, so the decoder must skip
/// frames that are already late instead of falling further and further
/// behind. Small frames, so it decodes them faster than they're due,
/// however busy the machine.
#[test]
fn keeps_up_when_frames_are_rarely_consumed() {
    let path = media(
        "small60.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=60:duration=4",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ],
    );
    let mut player = Player::open(Source::File(path), None, options(true, false));
    let start = Instant::now();
    // Each time, 24 frames are due: more than the 16 the queue holds.
    while start.elapsed() < Duration::from_millis(2800) {
        player.tick();
        std::thread::sleep(Duration::from_millis(400));
    }
    player.tick();
    let behind = player.position() - player.frame_pts().unwrap();
    assert!(behind < 0.3, "video is {behind:.2}s behind the clock");
}

#[test]
fn decodes_on_the_gpu_when_available() {
    let path = media(
        "video640.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=640x360:rate=30:duration=3",
            "-c:v",
            "libx264",
            "-g",
            "60",
            "-pix_fmt",
            "yuv420p",
        ],
    );
    let mut player = Player::open(
        Source::File(path),
        None,
        PlayerOptions {
            hardware_decoding: true,
            ..options(false, false)
        },
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    let info = player.info().unwrap();
    assert_eq!(info.video_size, Some((640, 360)));
    if cfg!(target_os = "macos") {
        assert_eq!(info.hardware, Some("VideoToolbox"));
    } else {
        eprintln!("hardware decoding: {:?}", info.hardware);
    }
    // Seeking flushes the hardware decoder and still lands on the right frame.
    for target in [1.5, 0.2, 2.9] {
        player.seek(target);
        assert!(
            wait_until(Duration::from_secs(5), || {
                player.tick();
                player
                    .frame_pts()
                    .is_some_and(|pts| (pts - target).abs() < 0.04)
            }),
            "seek to {target} landed on {:?}",
            player.frame_pts()
        );
    }
}

/// Hardware-decoded frames stay on the GPU and are drawn by gpui's surface
/// shader, which always converts with the BT.601 matrix from full-range NV12.
/// Whatever the video's own matrix and range, they must come out in the same
/// colors as the same frame converted to BGRA in software.
#[cfg(target_os = "macos")]
#[test]
fn surfaces_match_software_colors() {
    use super::picture::Picture;

    let cases: &[(&str, &str, &[&str])] = &[
        // Untagged video is assumed BT.601 below 720 lines and BT.709 from there.
        ("colors-sd.mp4", "640x360", &["-pix_fmt", "yuv420p"]),
        ("colors-hd.mp4", "1280x720", &["-pix_fmt", "yuv420p"]),
        (
            "colors-709-sd.mp4",
            "640x360",
            &["-pix_fmt", "yuv420p", "-colorspace", "bt709"],
        ),
        (
            "colors-601-hd.mp4",
            "1280x720",
            &["-pix_fmt", "yuv420p", "-colorspace", "smpte170m"],
        ),
        (
            "colors-full.mp4",
            "640x360",
            &["-pix_fmt", "yuvj420p", "-color_range", "pc"],
        ),
    ];
    for &(name, size, extra) in cases {
        let source = format!("testsrc2=size={size}:rate=30:duration=1");
        let mut args = vec!["-f", "lavfi", "-i", &source, "-c:v", "libx264", "-crf", "8"];
        args.extend_from_slice(extra);
        let path = media(name, &args);
        let Picture::Surface(surface) = first_frame(&path, true) else {
            panic!("{name}: hardware frames should stay on the GPU");
        };
        let Picture::Image(image) = first_frame(&path, false) else {
            panic!("{name}: software frames should be images");
        };

        let surface = surface_to_rgb(surface.buffer());
        let (width, height) = (surface.width, surface.height);
        let bgra = image.as_bytes(0).unwrap();
        assert_eq!(bgra.len(), width * height * 4, "{name}: sizes differ");
        let software = |x: usize, y: usize, c: usize| f64::from(bgra[(y * width + x) * 4 + 2 - c]);

        // Compare 8x8 block averages. VideoToolbox resamples chroma a little
        // differently from swscale while converting, which shows along sharp
        // color edges (the timecode, the thin rainbow line), so judge the bulk
        // of the picture. Done right, 90% of blocks are within about a level;
        // a wrong matrix puts that at 20 levels or more.
        let (mean, p90) =
            block_differences(width, height, |x, y, c| surface.get(x, y, c), software);
        assert!(
            mean < 1.5 && p90 < 3.0,
            "{name}: colors differ by {mean:.2} levels on average, {p90:.2} at the 90th percentile"
        );
    }
}

/// A surface's buffer carries the gamma it's drawn with, set as it's drawn
/// and only when that changed: one Picture in Picture had is left as it is.
#[cfg(target_os = "macos")]
#[test]
fn a_surface_is_tagged_with_its_gamma_only_when_that_changed() {
    use core_foundation::base::TCFType;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_video::buffer::{
        CVAttachmentMode, TCVBuffer, kCVAttachmentMode_ShouldNotPropagate,
        kCVAttachmentMode_ShouldPropagate,
    };

    use super::picture::{self, Picture};
    use super::videotoolbox::GAMMA_ATTACHMENT;

    let source = "testsrc2=size=640x360:rate=30:duration=1";
    let args = [
        "-f", "lavfi", "-i", source, "-c:v", "libx264", "-pix_fmt", "yuv420p",
    ];
    let path = media("gamma-tagged.mp4", &args);
    let Picture::Surface(surface) = first_frame(&path, true) else {
        panic!("hardware frames should stay on the GPU");
    };
    let buffer = surface.buffer().as_buffer();
    let key = CFString::from_static_string(GAMMA_ATTACHMENT);
    let tagged = || {
        let mut mode: CVAttachmentMode = kCVAttachmentMode_ShouldPropagate;
        let gamma = buffer
            .get_attachment(&key, Some(&mut mode))
            .and_then(|value| value.downcast::<CFNumber>())
            .and_then(|number| number.to_f32());
        (gamma, mode)
    };
    let tag = |gamma: f32| {
        let gamma = CFNumber::from(gamma).as_CFType();
        buffer.set_attachment(&key, &gamma, kCVAttachmentMode_ShouldPropagate);
    };
    let gamma = picture::gamma(false);
    let drawn = (Some(gamma), kCVAttachmentMode_ShouldNotPropagate);

    surface.for_drawing();
    assert_eq!(tagged(), drawn);

    // Already its gamma, though set another way: left as it is.
    tag(gamma);
    surface.for_drawing();
    let kept = (Some(gamma), kCVAttachmentMode_ShouldPropagate);
    assert_eq!(tagged(), kept, "set again");

    // Another: set to its own.
    tag(gamma + 1.);
    surface.for_drawing();
    assert_eq!(tagged(), drawn);
}

/// The first frame of the video at `path`, decoded on the GPU or in
/// software.
#[cfg(target_os = "macos")]
fn first_frame(path: &std::path::Path, hardware: bool) -> super::picture::Picture {
    let mut player = Player::open(
        Source::File(path.to_path_buf()),
        None,
        PlayerOptions {
            hardware_decoding: hardware,
            ..options(false, false)
        },
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    player.frame().unwrap()
}

/// How far apart two pictures of `width` by `height` are, by the averages
/// of 8x8 blocks of each channel (0-255) of `a` and `b`: the mean
/// difference, and the 90th percentile.
#[cfg(target_os = "macos")]
fn block_differences(
    width: usize,
    height: usize,
    a: impl Fn(usize, usize, usize) -> f64,
    b: impl Fn(usize, usize, usize) -> f64,
) -> (f64, f64) {
    let mut diffs = Vec::new();
    for by in (0..height).step_by(8) {
        for bx in (0..width).step_by(8) {
            let (ys, xs) = (by..(by + 8).min(height), bx..(bx + 8).min(width));
            let n = (ys.len() * xs.len()) as f64;
            for c in 0..3 {
                let (mut sum_a, mut sum_b) = (0.0, 0.0);
                for y in ys.clone() {
                    for x in xs.clone() {
                        sum_a += a(x, y, c);
                        sum_b += b(x, y, c);
                    }
                }
                diffs.push((sum_a - sum_b).abs() / n);
            }
        }
    }
    diffs.sort_by(f64::total_cmp);
    let mean = diffs.iter().sum::<f64>() / diffs.len() as f64;
    let p90 = diffs[diffs.len() * 9 / 10];
    (mean, p90)
}

/// RGB (0-255) of a surface as gpui's Metal shader computes it.
#[cfg(target_os = "macos")]
struct SurfaceRgb {
    width: usize,
    height: usize,
    rgb: Vec<f64>,
}

#[cfg(target_os = "macos")]
impl SurfaceRgb {
    fn get(&self, x: usize, y: usize, c: usize) -> f64 {
        self.rgb[(y * self.width + x) * 3 + c]
    }
}

#[cfg(target_os = "macos")]
fn surface_to_rgb(buffer: &core_video::pixel_buffer::CVPixelBuffer) -> SurfaceRgb {
    use core_video::pixel_buffer::{
        kCVPixelBufferLock_ReadOnly, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    };

    assert_eq!(
        buffer.get_pixel_format(),
        kCVPixelFormatType_420YpCbCr8BiPlanarFullRange
    );
    let (width, height) = (buffer.get_width(), buffer.get_height());
    assert_eq!(buffer.lock_base_address(kCVPixelBufferLock_ReadOnly), 0);
    let plane = |index: usize| {
        let rows = buffer.get_height_of_plane(index);
        let stride = buffer.get_bytes_per_row_of_plane(index);
        // SAFETY: the buffer is locked, and a plane is `rows * stride` bytes.
        unsafe {
            std::slice::from_raw_parts(
                buffer.get_base_address_of_plane(index) as *const u8,
                rows * stride,
            )
        }
    };
    let (luma, chroma) = (plane(0), plane(1));
    let luma_stride = buffer.get_bytes_per_row_of_plane(0);
    let chroma_stride = buffer.get_bytes_per_row_of_plane(1);
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let l = f64::from(luma[y * luma_stride + x]) / 255.0;
            let i = (y / 2) * chroma_stride + (x / 2) * 2;
            let cb = f64::from(chroma[i]) / 255.0;
            let cr = f64::from(chroma[i + 1]) / 255.0;
            // gpui's `surface_fragment` matrix.
            let r = l + 1.402 * cr - 0.701;
            let g = l - 0.3441 * cb - 0.7141 * cr + 0.5291;
            let b = l + 1.772 * cb - 0.886;
            rgb.extend([r, g, b].map(|v| v.clamp(0.0, 1.0) * 255.0));
        }
    }
    buffer.unlock_base_address(kCVPixelBufferLock_ReadOnly);
    SurfaceRgb { width, height, rgb }
}

/// HDR video on the GPU becomes 10-bit BT.2020 surfaces tagged with their
/// transfer function, which Plays' gpui draws with its HDR shader. For an SDR
/// screen that shader must give the same picture as the CPU's tone mapping of
/// the same frame decoded in software.
#[cfg(target_os = "macos")]
#[test]
fn hdr_surfaces_match_software_tone_mapping() {
    use core_foundation::base::TCFType;
    use core_video::buffer::CVBufferGetAttachment;
    use core_video::image_buffer::{
        CVTransferFunctionGetIntegerCodePointForString, kCVImageBufferTransferFunctionKey,
    };
    use core_video::pixel_buffer::kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange;

    use super::hdr::Transfer;
    use super::picture::Picture;

    let cases = [
        ("hdr10.mp4", "smpte2084", Transfer::Pq, 16),
        ("hlg.mp4", "arib-std-b67", Transfer::Hlg, 18),
    ];
    for (name, trc, transfer, code_point) in cases {
        let x265 = format!("log-level=error:colorprim=bt2020:transfer={trc}:colormatrix=bt2020nc");
        let args = [
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=30:duration=1",
            "-c:v",
            "libx265",
            "-tag:v",
            "hvc1",
            "-crf",
            "8",
            "-pix_fmt",
            "yuv420p10le",
            "-color_primaries",
            "bt2020",
            "-color_trc",
            trc,
            "-colorspace",
            "bt2020nc",
            "-x265-params",
            &x265,
        ];
        let path = media(name, &args);
        let Picture::Surface(surface) = first_frame(&path, true) else {
            panic!("{name}: hardware frames should stay on the GPU");
        };
        let Picture::Image(image) = first_frame(&path, false) else {
            panic!("{name}: software frames should be images");
        };

        let buffer = surface.buffer();
        assert_eq!(
            buffer.get_pixel_format(),
            kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
            "{name}: HDR surfaces should be 10-bit"
        );
        // SAFETY: reading a tag of a valid buffer.
        let tagged = unsafe {
            let value = CVBufferGetAttachment(
                buffer.as_concrete_TypeRef(),
                kCVImageBufferTransferFunctionKey,
                std::ptr::null_mut(),
            );
            (!value.is_null()).then(|| CVTransferFunctionGetIntegerCodePointForString(value.cast()))
        };
        assert_eq!(tagged, Some(code_point), "{name}: transfer function tag");

        let surface = hdr_surface_to_sdr(buffer, transfer);
        let (width, height) = (surface.width, surface.height);
        let bgra = image.as_bytes(0).unwrap();
        assert_eq!(bgra.len(), width * height * 4, "{name}: sizes differ");
        let software = |x: usize, y: usize, c: usize| f64::from(bgra[(y * width + x) * 4 + 2 - c]);

        // As in `surfaces_match_software_colors`, compare 8x8 block averages.
        let (mean, p90) =
            block_differences(width, height, |x, y, c| surface.get(x, y, c), software);
        assert!(
            mean < 1.5 && p90 < 3.0,
            "{name}: colors differ by {mean:.2} levels on average, {p90:.2} at the 90th percentile"
        );
    }
}

/// RGB (0-255) of a 10-bit HDR surface as gpui's `surface_hdr_fragment`
/// computes it for an SDR screen (EDR headroom 1).
#[cfg(target_os = "macos")]
fn hdr_surface_to_sdr(
    buffer: &core_video::pixel_buffer::CVPixelBuffer,
    transfer: super::hdr::Transfer,
) -> SurfaceRgb {
    use core_video::pixel_buffer::kCVPixelBufferLock_ReadOnly;

    use super::hdr::{
        REFERENCE_WHITE_NITS, Transfer, bt2020_to_bt709, hlg_scene_to_nits, hlg_to_scene,
        pq_to_nits, srgb_encode, tone_map,
    };

    let (width, height) = (buffer.get_width(), buffer.get_height());
    assert_eq!(buffer.lock_base_address(kCVPixelBufferLock_ReadOnly), 0);
    let plane = |index: usize| {
        let rows = buffer.get_height_of_plane(index);
        let stride = buffer.get_bytes_per_row_of_plane(index);
        // SAFETY: the buffer is locked, and a plane is `rows * stride` bytes
        // of 16-bit samples.
        unsafe {
            std::slice::from_raw_parts(
                buffer.get_base_address_of_plane(index) as *const u16,
                rows * stride / 2,
            )
        }
    };
    let (luma, chroma) = (plane(0), plane(1));
    let luma_stride = buffer.get_bytes_per_row_of_plane(0) / 2;
    let chroma_stride = buffer.get_bytes_per_row_of_plane(1) / 2;
    // BT.2020, video range; the 10 bits are the top ones of each sample.
    let (kr, kb) = (0.2627f32, 0.0593f32);
    let kg = 1.0 - kr - kb;
    let code = |sample: u16| f32::from(sample >> 6);
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let l = (code(luma[y * luma_stride + x]) - 64.0) / 876.0;
            let i = (y / 2) * chroma_stride + (x / 2) * 2;
            let cb = (code(chroma[i]) - 512.0) / 896.0;
            let cr = (code(chroma[i + 1]) - 512.0) / 896.0;
            let signal = [
                l + 2.0 * (1.0 - kr) * cr,
                l - 2.0 * kb * (1.0 - kb) / kg * cb - 2.0 * kr * (1.0 - kr) / kg * cr,
                l + 2.0 * (1.0 - kb) * cb,
            ]
            .map(|v| v.clamp(0.0, 1.0));
            let light = match transfer {
                Transfer::Pq => signal.map(|s| pq_to_nits(s) / REFERENCE_WHITE_NITS),
                Transfer::Hlg => hlg_scene_to_nits(signal.map(hlg_to_scene))
                    .map(|nits| nits / REFERENCE_WHITE_NITS),
            };
            let mapped = tone_map(bt2020_to_bt709(light), 1.0);
            rgb.extend(mapped.map(|v| f64::from(srgb_encode(v.clamp(0.0, 1.0))) * 255.0));
        }
    }
    buffer.unlock_base_address(kCVPixelBufferLock_ReadOnly);
    SurfaceRgb { width, height, rgb }
}

/// Dolby Vision profile 5 can't be made with the ffmpeg tool, so this runs
/// on a file of your own: `PLAYS_DOLBY_VISION_SAMPLE=path cargo test dolby`.
/// Its surfaces must carry the frame's reshaping and, through the Dolby
/// Vision shader's math, show what software decoding shows. Both pictures
/// are saved as PNGs next to the generated test media.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "needs a Dolby Vision profile 5 file named by PLAYS_DOLBY_VISION_SAMPLE"]
fn dolby_vision_surfaces_match_software() {
    use core_foundation::base::TCFType;
    use core_foundation::data::{CFData, CFDataRef};
    use core_foundation::string::CFString;
    use core_video::buffer::CVBufferGetAttachment;

    use super::dovi::ShaderParams;
    use super::picture::Picture;
    use super::videotoolbox::DOLBY_VISION_ATTACHMENT;

    let path = std::env::var_os("PLAYS_DOLBY_VISION_SAMPLE")
        .map(PathBuf::from)
        .expect("PLAYS_DOLBY_VISION_SAMPLE names a Dolby Vision profile 5 file");
    ffmpeg_next::init().unwrap();
    let out_dir = kept();
    std::fs::create_dir_all(&out_dir).unwrap();
    for share in [0.2, 0.5] {
        let frame_at = |hardware| {
            let mut player = Player::open(
                Source::File(path.clone()),
                None,
                PlayerOptions {
                    hardware_decoding: hardware,
                    max_short_side: Some(720),
                    ..options(false, false)
                },
            );
            assert!(wait_until(Duration::from_secs(10), || {
                player.tick();
                player.duration().is_some()
            }));
            let time = (player.duration().unwrap() * share).round();
            player.seek(time);
            assert!(wait_until(Duration::from_secs(30), || {
                player.tick();
                player.frame_pts().is_some_and(|pts| pts >= time - 1.0)
            }));
            (player.frame().unwrap(), time)
        };
        let (Picture::Surface(surface), time) = frame_at(true) else {
            panic!("hardware frames should stay on the GPU");
        };
        let (Picture::Image(image), _) = frame_at(false) else {
            panic!("software frames should be images");
        };

        let buffer = surface.buffer();
        let key = CFString::from_static_string(DOLBY_VISION_ATTACHMENT);
        // SAFETY: reading an attachment of a valid buffer.
        let params = unsafe {
            let value = CVBufferGetAttachment(
                buffer.as_concrete_TypeRef(),
                key.as_concrete_TypeRef(),
                std::ptr::null_mut(),
            );
            assert!(!value.is_null(), "surfaces should carry the reshaping");
            ShaderParams::from_bytes(CFData::wrap_under_get_rule(value as CFDataRef).bytes())
        };

        let gpu = dovi_surface_to_sdr(buffer, &params);
        let (width, height) = (gpu.width, gpu.height);
        let bgra = image.as_bytes(0).unwrap();
        assert_eq!(bgra.len(), width * height * 4, "sizes differ");
        let to_png = |name: &str, rgb: &dyn Fn(usize) -> [u8; 3]| {
            let pixels = (0..width * height).flat_map(rgb).collect();
            let png = out_dir.join(format!("dolby-vision-{time}-{name}.png"));
            image::RgbImage::from_raw(width as u32, height as u32, pixels)
                .unwrap()
                .save(&png)
                .unwrap();
            eprintln!("{}", png.display());
        };
        to_png("gpu", &|i| {
            std::array::from_fn(|c| gpu.rgb[i * 3 + c].round() as u8)
        });
        to_png("cpu", &|i| [bgra[i * 4 + 2], bgra[i * 4 + 1], bgra[i * 4]]);

        let (mean, p90) = block_differences(
            width,
            height,
            |x, y, c| gpu.get(x, y, c),
            |x, y, c| f64::from(bgra[(y * width + x) * 4 + 2 - c]),
        );
        eprintln!("{time}s: {mean:.2} levels apart on average, {p90:.2} at the 90th percentile");
        assert!(
            mean < 1.5 && p90 < 3.0,
            "{time}s: colors differ by {mean:.2} levels on average, {p90:.2} at the 90th percentile"
        );
    }
}

/// RGB (0-255) of a Dolby Vision surface as gpui's `surface_dovi_fragment`
/// computes it for an SDR screen, with the frame's reshaping `params`.
#[cfg(target_os = "macos")]
fn dovi_surface_to_sdr(
    buffer: &core_video::pixel_buffer::CVPixelBuffer,
    params: &super::dovi::ShaderParams,
) -> SurfaceRgb {
    use core_video::pixel_buffer::kCVPixelBufferLock_ReadOnly;

    use super::hdr::{REFERENCE_WHITE_NITS, bt2020_to_bt709, srgb_encode, tone_map};

    let (width, height) = (buffer.get_width(), buffer.get_height());
    assert_eq!(buffer.lock_base_address(kCVPixelBufferLock_ReadOnly), 0);
    let plane = |index: usize| {
        let rows = buffer.get_height_of_plane(index);
        let stride = buffer.get_bytes_per_row_of_plane(index) / 2;
        // SAFETY: the buffer is locked, and a plane is `rows` rows of
        // `stride` 16-bit samples.
        let samples = unsafe {
            std::slice::from_raw_parts(
                buffer.get_base_address_of_plane(index) as *const u16,
                rows * stride,
            )
        };
        (samples, stride)
    };
    let ((luma, luma_stride), (chroma, chroma_stride)) = (plane(0), plane(1));
    let mut rgb = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        for x in 0..width {
            let i = (y / 2) * chroma_stride + (x / 2) * 2;
            let samples = [luma[y * luma_stride + x], chroma[i], chroma[i + 1]]
                .map(|s| f32::from(s) / 65536.0);
            let light = params
                .light(samples)
                .map(|nits| nits / REFERENCE_WHITE_NITS);
            let mapped = tone_map(bt2020_to_bt709(light), 1.0);
            rgb.extend(mapped.map(|v| f64::from(srgb_encode(v.clamp(0.0, 1.0)) * 255.0)));
        }
    }
    buffer.unlock_base_address(kCVPixelBufferLock_ReadOnly);
    SurfaceRgb { width, height, rgb }
}

// ---------------------------------------------------------------------------
// Network streams, served from a tiny HTTP server on localhost.

mod http {
    use std::io::{BufRead, BufReader, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// The paths asked for, in order.
    pub type Asked = Arc<Mutex<Vec<String>>>;

    /// Serve files from `dir`, with byte ranges like a real web server. With
    /// `stall`, ranges aren't offered and each response pauses for that long
    /// halfway through, like a congested connection.
    pub fn serve(dir: PathBuf, stall: Option<Duration>) -> SocketAddr {
        serve_noting(dir, stall).0
    }

    /// As `serve`, noting each path asked for.
    pub fn serve_noting(dir: PathBuf, stall: Option<Duration>) -> (SocketAddr, Asked) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let asked = Asked::default();
        let noting = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let dir = dir.clone();
                let noting = noting.clone();
                std::thread::spawn(move || respond(stream, &dir, stall, &noting));
            }
        });
        (addr, asked)
    }

    fn respond(
        mut stream: TcpStream,
        dir: &std::path::Path,
        stall: Option<Duration>,
        asked: &Asked,
    ) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request = String::new();
        if reader.read_line(&mut request).is_err() {
            return;
        }
        let mut start = 0;
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
            if let Some(range) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                start = range
                    .split('-')
                    .next()
                    .and_then(|n| n.trim().parse().ok())
                    .unwrap_or(0);
            }
            line.clear();
        }
        let path = request.split_whitespace().nth(1).unwrap_or("/");
        asked.lock().unwrap().push(path.to_owned());
        let Ok(body) = std::fs::read(dir.join(path.trim_start_matches('/'))) else {
            let _ = stream.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            return;
        };
        let len = body.len();
        let header = if stall.is_some() {
            format!("HTTP/1.1 200 OK\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n")
        } else if start > 0 {
            if start >= len {
                let _ = stream.write_all(format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{len}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
                return;
            }
            format!(
                "HTTP/1.1 206 Partial Content\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {start}-{}/{len}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                len - 1,
                len - start
            )
        } else {
            format!(
                "HTTP/1.1 200 OK\r\nAccept-Ranges: bytes\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
            )
        };
        let body = if stall.is_some() {
            &body[..]
        } else {
            &body[start..]
        };
        let _ = stream.write_all(header.as_bytes());
        let (first, rest) = body.split_at(if stall.is_some() {
            body.len() / 2
        } else {
            body.len()
        });
        let _ = stream.write_all(first);
        if let Some(stall) = stall {
            let _ = stream.flush();
            std::thread::sleep(stall);
        }
        let _ = stream.write_all(rest);
    }
}

/// How `stream_media` makes its video.
const STREAM: &[&str] = &[
    "-f",
    "lavfi",
    "-i",
    "testsrc=size=320x240:rate=25:duration=4",
    "-f",
    "lavfi",
    "-i",
    "sine=frequency=440:sample_rate=48000:duration=4",
    "-c:v",
    "libx264",
    "-preset",
    "ultrafast",
    "-g",
    "25",
    "-pix_fmt",
    "yuv420p",
    "-c:a",
    "aac",
    "-ac",
    "2",
];

/// How `stream_media` makes the HLS version of its video.
const HLS: &[&str] = &[
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
];

/// A short video with sound, as MPEG-TS, plus an HLS version of it in `hls/`.
fn stream_media() -> PathBuf {
    let ts = media("stream.ts", STREAM);
    let dir = ts.parent().unwrap().to_path_buf();
    made_in(&dir, "hls", &format!("{STREAM:?} {HLS:?}"), |hls| {
        std::fs::create_dir(hls).map_err(|err| err.to_string())?;
        let status = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&ts)
            .args(HLS)
            .arg("-hls_segment_filename")
            .arg(hls.join("segment%d.ts"))
            .arg(hls.join("vod.m3u8"))
            .status()
            .map_err(|err| err.to_string())?;
        if !status.success() {
            return Err("ffmpeg couldn't make the HLS version".into());
        }
        // The same segments without an end: what a live stream looks like.
        let vod = std::fs::read_to_string(hls.join("vod.m3u8")).map_err(|err| err.to_string())?;
        let live: String = vod
            .lines()
            .filter(|l| !l.starts_with("#EXT-X-ENDLIST") && !l.starts_with("#EXT-X-PLAYLIST-TYPE"))
            .map(|l| format!("{l}\n"))
            .collect();
        std::fs::write(hls.join("live.m3u8"), live).map_err(|err| err.to_string())
    });
    dir
}

#[test]
fn plays_a_file_over_http() {
    let dir = stream_media();
    let addr = http::serve(dir, None);
    let mixer = Mixer::detached(48_000);
    let source = Source::parse_url(&format!("http://{addr}/stream.ts")).unwrap();
    let mut player = Player::open(source, Some(mixer.clone()), options(true, false));
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(20), |_| false);
    assert!(player.error().is_none(), "{:?}", player.error());
    let info = player.info().unwrap();
    assert!(info.has_video && info.has_audio && !info.live, "{info:?}");
    assert!((info.duration.unwrap() - 4.0).abs() < 0.2, "{info:?}");
    assert!(player.frame_pts().is_some());
    assert!(!player.is_playing(), "the stream should have ended");
    // Four seconds of audio (AAC priming may trim a frame).
    assert!(
        (heard as i64 - 4 * 48_000).abs() < 4_800,
        "heard {heard} frames"
    );
}

#[test]
fn plays_and_seeks_hls() {
    let dir = stream_media();
    let addr = http::serve(dir, None);
    let source = Source::parse_url(&format!("http://{addr}/hls/vod.m3u8")).unwrap();
    let mut player = Player::open(source, None, options(false, false));
    assert!(
        wait_until(Duration::from_secs(10), || {
            player.tick();
            player.frame_pts().is_some()
        }),
        "{:?}",
        player.error()
    );
    let info = player.info().unwrap();
    assert!(!info.live);
    assert!((info.duration.unwrap() - 4.0).abs() < 0.2, "{info:?}");
    player.seek(2.5);
    assert!(
        wait_until(Duration::from_secs(10), || {
            player.tick();
            player
                .frame_pts()
                .is_some_and(|pts| (pts - 2.5).abs() < 0.045)
        }),
        "seek landed on {:?}",
        player.frame_pts()
    );
}

#[test]
fn treats_playlists_without_an_end_as_live() {
    let dir = stream_media();
    let addr = http::serve(dir, None);
    let source = Source::parse_url(&format!("http://{addr}/hls/live.m3u8")).unwrap();
    let mut player = Player::open(source, None, options(true, false));
    assert!(
        wait_until(Duration::from_secs(10), || {
            player.tick();
            player.frame_pts().is_some()
        }),
        "{:?}",
        player.error()
    );
    assert!(player.is_live());
    assert_eq!(player.duration(), None);
    // Live streams can't seek; the request is ignored.
    let before = player.position();
    player.seek(before + 30.0);
    assert!(player.position() < before + 1.0);
    // Closing must not wait for the playlist to be reloaded.
    let start = Instant::now();
    drop(player);
    assert!(start.elapsed() < Duration::from_millis(100));
}

#[test]
fn waits_for_a_stalled_stream_and_resumes() {
    let dir = stream_media();
    let addr = http::serve(dir, Some(Duration::from_secs(3)));
    let mixer = Mixer::detached(48_000);
    let source = Source::parse_url(&format!("http://{addr}/stream.ts")).unwrap();
    let mut player = Player::open(source, Some(mixer.clone()), options(true, false));
    let mut heard = 0;
    let mut stalled = false;
    let mut buffer = vec![0.0f32; 512 * 2];
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        player.tick();
        if !player.is_playing() {
            break;
        }
        stalled |= heard > 0 && player.is_buffering();
        mixer.mix(&mut buffer, 0.0, Instant::now());
        heard += buffer
            .as_chunks::<2>()
            .0
            .iter()
            .filter(|f| f[0].abs() > 1e-6)
            .count();
        std::thread::sleep(Duration::from_micros(300));
    }
    assert!(stalled, "the player never noticed the stall");
    assert!(!player.is_playing(), "the stream should have ended");
    // Everything was heard despite the pause in the middle: the player held
    // the clock instead of skipping ahead.
    assert!(
        (heard as i64 - 4 * 48_000).abs() < 4_800,
        "heard {heard} frames"
    );
}

#[test]
fn reports_unreachable_streams() {
    ffmpeg_next::init().unwrap();
    // Nothing listens on port 9 (discard) on test machines.
    let source = Source::parse_url("http://127.0.0.1:9/missing.ts").unwrap();
    let mut player = Player::open(source, None, options(true, false));
    assert!(wait_until(Duration::from_secs(10), || {
        player.tick();
        player.error().is_some()
    }));
    let error = player.error().unwrap();
    assert!(
        error.contains("could not open http://127.0.0.1:9/missing.ts"),
        "{error}"
    );
}

/// A server that accepts the connection and then never answers. Closing the
/// player must abort the blocked read instead of leaving threads behind.
#[test]
fn closing_aborts_a_hanging_connection() {
    use std::io::Read;
    use std::net::TcpListener;

    ffmpeg_next::init().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let source = Source::parse_url(&format!("http://{addr}/hang.ts")).unwrap();
    let player = Player::open(source, None, options(true, false));
    let (mut connection, _) = listener.accept().unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    // Read the request; FFmpeg now waits for a response that never comes.
    let mut request = [0u8; 4096];
    assert!(connection.read(&mut request).unwrap() > 0);

    drop(player);
    let start = Instant::now();
    // The connection is closed (read returns 0) once FFmpeg gives up.
    let closed = loop {
        match connection.read(&mut request) {
            Ok(0) => break true,
            Ok(_) => continue,
            Err(_) => break false,
        }
    };
    assert!(closed, "the connection was left open");
    assert!(start.elapsed() < Duration::from_secs(2));
}

/// Screenshots decode the frame shown again, at full size whatever the
/// resolution limit, and find exactly that frame though it isn't a keyframe.
#[test]
fn stills_are_the_frame_shown_at_full_size() {
    use super::still;

    // Each frame a flat grey a few levels brighter than the one before.
    let path = media(
        "frames.mkv",
        &[
            "-f",
            "lavfi",
            "-i",
            "color=black:size=640x360:rate=25:duration=2,format=yuv420p,\
             geq=lum='16+4*N':cb=128:cr=128",
            "-c:v",
            "libx264",
            "-g",
            "50",
            "-crf",
            "1",
            "-pix_fmt",
            "yuv420p",
        ],
    );
    let source = Source::File(path);
    let mut player = Player::open(
        source.clone(),
        None,
        PlayerOptions {
            max_short_side: Some(180),
            ..options(false, false)
        },
    );
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.frame_pts().is_some()
    }));
    player.seek(1.0);
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player
            .frame_pts()
            .is_some_and(|pts| (pts - 1.0).abs() < 0.01)
    }));
    assert_eq!(player.frame().unwrap().size(), (320, 180));
    let stream = player.info().unwrap().video_track.unwrap();
    let timestamp = player.frame_timestamp().unwrap();

    let grey = |still: &still::Still| f64::from(still.pixels[still.pixels.len() / 2][1]) / 257.0;
    // Frame `n` in full-range RGB.
    let frame = |n: f64| 4.0 * n * 255.0 / 219.0;
    let shot = still::grab(&source, None, stream, timestamp, false).unwrap();
    assert_eq!((shot.width, shot.height, shot.hdr), (640, 360, None));
    assert!((grey(&shot) - frame(25.0)).abs() < 1.5, "{}", grey(&shot));
    // The first frame, which is a keyframe, too.
    let first = still::grab(&source, None, stream, 0, false).unwrap();
    assert!(grey(&first) < 1.5, "{}", grey(&first));
}

/// HDR stills either look as an SDR screen shows them or keep their PQ
/// signal, which tone maps to the same picture.
#[test]
fn hdr_stills_keep_their_signal() {
    use super::hdr::{REFERENCE_WHITE_NITS, ToneMapper, Transfer, pq_to_nits};
    use super::still;

    let path = media(
        "still-pq.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=25:duration=1",
            "-c:v",
            "libx265",
            "-crf",
            "8",
            "-pix_fmt",
            "yuv420p10le",
            "-color_primaries",
            "bt2020",
            "-color_trc",
            "smpte2084",
            "-colorspace",
            "bt2020nc",
            "-x265-params",
            "log-level=error:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc",
        ],
    );
    let source = Source::File(path);
    let sdr = still::grab(&source, None, 0, 0, false).unwrap();
    let hdr = still::grab(&source, None, 0, 0, true).unwrap();
    assert_eq!(sdr.hdr, None);
    assert_eq!(hdr.hdr, Some(Transfer::Pq));
    assert_eq!((hdr.width, hdr.height), (sdr.width, sdr.height));
    let mapper = ToneMapper::new(Transfer::Pq);
    let mut worst = 0;
    for (kept, shown) in hdr.pixels.iter().zip(&sdr.pixels).step_by(97) {
        let light = kept.map(|s| pq_to_nits(f32::from(s) / 65535.0) / REFERENCE_WHITE_NITS);
        let [b, g, r, _] = mapper.output(light);
        for (mapped, shown) in [r, g, b].into_iter().zip(shown) {
            worst = worst.max((i32::from(mapped) - i32::from(shown / 257)).abs());
        }
    }
    assert!(worst <= 3, "kept HDR tone maps {worst} levels off");
}
