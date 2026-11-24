//! Files and systems at the edges: absurd times and rates, sound whose
//! rate is known late or changes, a thread that won't start.

use std::time::Duration;

use super::made::media;
use super::{drain_audio, options, wait_until};
use crate::media::audio::Mixer;
use crate::media::decoder::{Shared, started};
use crate::media::picking::{Picked, Wanted};
use crate::media::player::{Player, PlayerOptions, wait_for};
use crate::media::source::Source;

#[test]
fn a_frame_due_further_off_than_a_duration_holds_is_waited_for_longest() {
    assert_eq!(wait_for(1e20), Duration::MAX);
    assert_eq!(wait_for(f64::INFINITY), Duration::MAX);
    assert_eq!(wait_for(0.5), Duration::from_millis(500));
    assert_eq!(wait_for(-3.0), Duration::ZERO);
    assert_eq!(wait_for(f64::NAN), Duration::ZERO);
}

/// A WAV saying its sound has one sample a second, in frames of 4096:
/// resampled, each would take gigabytes.
fn one_sample_a_second() -> Vec<u8> {
    let data = vec![0u8; 65_536];
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    // PCM, two channels, one sample a second, four bytes a second, in
    // blocks of 16 KiB, 16 bits a sample.
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&1u32.to_le_bytes());
    wav.extend_from_slice(&4u32.to_le_bytes());
    wav.extend_from_slice(&16_384u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    wav
}

#[test]
fn sound_with_too_few_samples_a_second_isnt_played() {
    let folder = crate::files::test_folder("too-few-samples");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("slow.wav");
    std::fs::write(&path, one_sample_a_second()).unwrap();
    let mixer = Some(Mixer::detached(48_000));
    let mut player = Player::open(Source::File(path), mixer, options(true, false));
    assert!(wait_until(Duration::from_secs(5), || {
        player.tick();
        player.error().is_some()
    }));
    let error = player.error().unwrap();
    assert!(error.contains("1 samples a second"), "{error}");
}

/// A film whose sound can't play plays its picture without it.
#[test]
fn a_film_whose_sound_has_too_few_samples_a_second_plays_its_picture() {
    let folder = crate::files::test_folder("film-too-few-samples");
    std::fs::create_dir_all(&folder).unwrap();
    let sound = folder.join("slow.wav");
    std::fs::write(&sound, one_sample_a_second()).unwrap();
    let film = folder.join("film.mkv");
    let made = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-f", "lavfi", "-i"])
        .arg("testsrc=size=160x120:rate=25:duration=1")
        .arg("-i")
        .arg(&sound)
        .args(["-c:v", "libx264", "-c:a", "copy", "-t", "1"])
        .arg(&film)
        .status()
        .unwrap();
    assert!(made.success());
    let mixer = Some(Mixer::detached(48_000));
    let mut player = Player::open(Source::File(film), mixer, options(true, false));
    assert!(
        wait_until(Duration::from_secs(5), || {
            player.tick();
            player.frame_pts().is_some()
        }),
        "{:?}",
        player.error()
    );
    let info = player.info().unwrap();
    assert!(info.has_video && !info.has_audio);
}

/// Sound starting after where FFmpeg looks as a film opens, picked: its
/// rate isn't known until its first frame, and it's opened all the same.
#[test]
fn sound_picked_whose_rate_isnt_known_yet_is_opened() {
    let film = media(
        "late-sound.ts",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=160x120:rate=25:duration=20",
            "-itsoffset",
            "12",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=8",
            "-map",
            "0",
            "-map",
            "1",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-c:a",
            "aac",
            "-f",
            "mpegts",
        ],
    );
    let picked = PlayerOptions {
        wanted: Wanted {
            picked: Picked {
                audio: Some(1),
                subtitles: None,
            },
            ..Wanted::default()
        },
        ..options(true, false)
    };
    let mixer = Some(Mixer::detached(48_000));
    let mut player = Player::open(Source::File(film), mixer, picked);
    assert!(
        wait_until(Duration::from_secs(5), || {
            player.tick();
            player.frame_pts().is_some()
        }),
        "{:?}",
        player.error()
    );
    let info = player.info().unwrap();
    assert!(info.has_audio, "{:?}", info.audio_track);
}

/// FLAC, stereo frames of 256 samples at a quarter of full scale, each
/// at its rate in `rates`; 0 is the rate its header says, 48 kHz. FFmpeg
/// takes each frame's rate from the frame.
fn flac_at(rates: &[u16]) -> Vec<u8> {
    const BLOCK: u16 = 256;
    let mut flac = b"fLaC".to_vec();
    // The last metadata block, the stream's, 34 bytes long: the least and
    // most samples a frame, its least and most bytes not known, then 48 kHz
    // in 20 bits, two channels, 16 bits a sample and the samples in 36,
    // and no MD5.
    flac.extend_from_slice(&[0x80, 0, 0, 34]);
    flac.extend_from_slice(&BLOCK.to_be_bytes());
    flac.extend_from_slice(&BLOCK.to_be_bytes());
    flac.extend_from_slice(&[0; 6]);
    let samples = u64::from(BLOCK) * u64::try_from(rates.len()).unwrap();
    let stream = (48_000u64 << 44) | (1 << 41) | (15 << 36) | samples;
    flac.extend_from_slice(&stream.to_be_bytes());
    flac.extend_from_slice(&[0; 16]);
    for (n, &rate) in rates.iter().enumerate() {
        // Sync; 256 samples, and the rate the header's or in Hz after the
        // frame's number; two channels apart, 16 bits a sample.
        let given = if rate == 0 { 0 } else { 0b1101 };
        let mut frame = vec![
            0xFF,
            0xF8,
            (0b1000 << 4) | given,
            (0b0001 << 4) | (0b100 << 1),
            u8::try_from(n).unwrap(),
        ];
        if rate != 0 {
            frame.extend_from_slice(&rate.to_be_bytes());
        }
        frame.push(u8::try_from(crc(&frame, 0x07, 8)).unwrap());
        // Each channel one sample, held.
        for _ in 0..2 {
            frame.push(0);
            frame.extend_from_slice(&8192i16.to_be_bytes());
        }
        let sum = u16::try_from(crc(&frame, 0x8005, 16)).unwrap();
        frame.extend_from_slice(&sum.to_be_bytes());
        flac.extend_from_slice(&frame);
    }
    flac
}

/// The CRC `bits` wide of `bytes`, by `poly`, from 0, as FLAC sums frames.
fn crc(bytes: &[u8], poly: u32, bits: u32) -> u32 {
    let top = 1 << (bits - 1);
    let mask = (1 << bits) - 1;
    bytes.iter().fold(0, |sum, &byte| {
        (0..8).fold(sum ^ (u32::from(byte) << (bits - 8)), |sum, _| {
            let shifted = sum << 1;
            (if sum & top == 0 {
                shifted
            } else {
                shifted ^ poly
            }) & mask
        })
    })
}

/// Sound whose rate falls below what plays partway through: what's after
/// is let go, not resampled.
#[test]
fn sound_whose_rate_falls_too_low_isnt_played_from_there() {
    let folder = crate::files::test_folder("rate-falls");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("falls.flac");
    std::fs::write(&path, flac_at(&[0, 0, 0, 0, 999, 999, 999, 999])).unwrap();
    let mixer = Mixer::detached(48_000);
    let mut player = Player::open(
        Source::File(path),
        Some(mixer.clone()),
        options(true, false),
    );
    let heard = drain_audio(&mut player, &mixer, Duration::from_secs(10), |_| false);
    // Four frames of 256 at 48 kHz: each of the rest, at 999 Hz, would be
    // about 12300.
    assert!((1..2048).contains(&heard), "heard {heard} frames");
}

#[test]
fn a_decoder_that_cant_start_says_so() {
    let shared = Shared::new(None, 1.0);
    started(
        Err(std::io::Error::other("no threads left")),
        &shared,
        "clip",
    );
    assert_eq!(
        shared.error().as_deref(),
        Some("couldn't start decoding: no threads left")
    );
}
