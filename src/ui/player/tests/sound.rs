use super::*;
use crate::media::details::{AudioDetails, ContainerDetails};

fn song() -> MediaInfo {
    MediaInfo {
        has_audio: true,
        audio_codec: Some("aac".to_owned()),
        title: Some("Рэп больше не кал".to_owned()),
        artist: Some("2h Company".to_owned()),
        album: Some("Искусство ухода за АК-47".to_owned()),
        container: ContainerDetails {
            format: "QuickTime / MOV".to_owned(),
            bit_rate: Some(294_204),
        },
        audio_details: Some(AudioDetails {
            profile: Some("LC".to_owned()),
            sample_rate: 44_100,
            channels: 2,
            layout: Some("stereo".to_owned()),
            sample_format: Some("fltp"),
            bits: None,
            bit_rate: Some(289_541),
        }),
        ..MediaInfo::default()
    }
}

#[test]
fn a_song_shows_its_title_artist_album_and_quality() {
    assert_eq!(
        SoundLines::of(&song(), &"01 Рэп больше не кал.m4a".into()),
        SoundLines {
            title: "Рэп больше не кал".into(),
            artist: Some("2h Company".to_owned()),
            album: Some("Искусство ухода за АК-47".to_owned()),
            quality: Some("AAC LC · 44.1 kHz · Stereo · 290 kb/s".to_owned()),
        }
    );
}

#[test]
fn a_file_with_no_tags_is_titled_by_its_name() {
    let info = MediaInfo {
        title: None,
        artist: None,
        album: None,
        ..song()
    };
    let lines = SoundLines::of(&info, &"tone.m4a".into());
    assert_eq!(lines.title, "tone.m4a");
    assert_eq!((lines.artist, lines.album), (None, None));
}

#[test]
fn the_album_artist_shows_by_the_album_only_when_not_the_artist() {
    let name = "song.flac".into();
    let by = |album_artist: &str| MediaInfo {
        album_artist: Some(album_artist.to_owned()),
        ..song()
    };
    let other = SoundLines::of(&by("Various Artists"), &name);
    assert_eq!(other.artist.as_deref(), Some("2h Company"));
    assert_eq!(
        other.album.as_deref(),
        Some("Искусство ухода за АК-47 — Various Artists")
    );
    let same = SoundLines::of(&by("2H COMPANY"), &name);
    assert_eq!(same.album.as_deref(), Some("Искусство ухода за АК-47"));
    // With no artist, the album artist is the artist.
    let no_artist = MediaInfo {
        artist: None,
        ..by("Various Artists")
    };
    let lines = SoundLines::of(&no_artist, &name);
    assert_eq!(lines.artist.as_deref(), Some("Various Artists"));
    assert_eq!(lines.album.as_deref(), Some("Искусство ухода за АК-47"));
    // With no album, it stands alone.
    let no_album = MediaInfo {
        album: None,
        ..by("Various Artists")
    };
    let lines = SoundLines::of(&no_album, &name);
    assert_eq!(lines.album.as_deref(), Some("Various Artists"));
}

#[test]
fn lossless_sound_says_its_bits_and_the_files_bit_rate() {
    let info = MediaInfo {
        audio_codec: Some("flac".to_owned()),
        container: ContainerDetails {
            format: "raw FLAC".to_owned(),
            bit_rate: Some(2_304_000),
        },
        audio_details: Some(AudioDetails {
            profile: None,
            sample_rate: 96_000,
            channels: 1,
            layout: Some("mono".to_owned()),
            sample_format: Some("s32"),
            bits: Some(24),
            bit_rate: None,
        }),
        ..song()
    };
    let lines = SoundLines::of(&info, &"song.flac".into());
    assert_eq!(
        lines.quality.as_deref(),
        Some("FLAC · 24-bit · 96 kHz · Mono · 2.3 Mb/s")
    );
}

#[test]
fn sound_with_no_layout_or_bit_rate_says_what_it_knows() {
    let info = MediaInfo {
        audio_codec: Some("pcm_s16le".to_owned()),
        container: ContainerDetails::default(),
        audio_details: Some(AudioDetails {
            channels: 6,
            layout: None,
            sample_rate: 48_000,
            bits: Some(16),
            ..AudioDetails::default()
        }),
        ..song()
    };
    let lines = SoundLines::of(&info, &"song.wav".into());
    assert_eq!(
        lines.quality.as_deref(),
        Some("PCM · 16-bit · 48 kHz · 6 channels")
    );
    let no_rate = MediaInfo {
        audio_details: Some(AudioDetails {
            sample_rate: 0,
            ..info.audio_details.clone().unwrap()
        }),
        ..info
    };
    assert_eq!(
        SoundLines::of(&no_rate, &"song.wav".into())
            .quality
            .as_deref(),
        Some("PCM · 16-bit · 6 channels")
    );
    let no_sound = MediaInfo {
        audio_details: None,
        ..song()
    };
    assert_eq!(SoundLines::of(&no_sound, &"song.wav".into()).quality, None);
}

#[test]
fn codecs_go_by_their_common_names() {
    for (codec, name) in [
        ("aac", "AAC"),
        ("mp3", "MP3"),
        ("opus", "Opus"),
        ("eac3", "E-AC-3"),
        ("pcm_f32le", "PCM"),
        ("dsd_lsbf", "DSD"),
        ("tta", "TTA"),
    ] {
        assert_eq!(codec_name(codec), name);
    }
}
