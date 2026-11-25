use super::*;

fn info(pairs: &[(&str, Value)]) -> Dictionary {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

#[test]
fn reads_what_the_tv_says_it_is_doing() {
    let playing = info(&[
        ("duration", 120.0.into()),
        ("position", 12.5.into()),
        ("rate", 1.0.into()),
        ("readyToPlay", true.into()),
    ]);
    assert_eq!(
        read_playback_info(&playing, false),
        PlaybackInfo {
            state: PlaybackState::Playing,
            position: 12.5,
            duration: Some(120.0),
        }
    );
    let mut paused = playing.clone();
    paused.insert("rate".into(), 0.0.into());
    assert_eq!(
        read_playback_info(&paused, true).state,
        PlaybackState::Paused
    );
    let mut stalled = playing.clone();
    stalled.insert("playbackBufferEmpty".into(), true.into());
    assert_eq!(
        read_playback_info(&stalled, true).state,
        PlaybackState::Loading
    );
    let mut at_end = paused;
    at_end.insert("position".into(), 119.8.into());
    assert_eq!(
        read_playback_info(&at_end, true).state,
        PlaybackState::Ended
    );
}

#[test]
fn saying_nothing_is_loading_before_it_played_and_the_end_after() {
    assert_eq!(
        read_playback_info(&Dictionary::new(), false).state,
        PlaybackState::Loading
    );
    assert_eq!(
        read_playback_info(&Dictionary::new(), true).state,
        PlaybackState::Ended
    );
    let not_ready = info(&[("duration", 60.0.into()), ("readyToPlay", false.into())]);
    assert_eq!(
        read_playback_info(&not_ready, false).state,
        PlaybackState::Loading
    );
}

/// The item last given.
const ITEM: &str = "6C2A2E71-0D44-4F4B-8B7E-6F0D5E9A1C3B";

/// A time as commands say it: `value` over `timescale`, with `flags`.
fn cm(value: i64, timescale: i64, flags: i64) -> Value {
    info(&[
        ("value", value.into()),
        ("timescale", timescale.into()),
        ("flags", flags.into()),
        ("epoch", 0.into()),
    ])
    .into()
}

#[test]
fn times_go_to_the_millisecond_and_are_read_only_when_valid() {
    assert_eq!(time(12.5), cm(12_500, 1000, 1));
    assert_eq!(seconds(Some(&time(0.0004))), Some(0.0));
    assert_eq!(
        seconds(Some(&cm(5_026_218_701, 1_000_000_000, 1))),
        Some(5.026_218_701)
    );
    // Rounded, as durations are.
    assert_eq!(seconds(Some(&cm(1_800_011, 1000, 3))), Some(1800.011));
    assert_eq!(seconds(Some(&cm(5, 1, 0))), None, "not valid");
    for flags in [0x05, 0x09, 0x11] {
        assert_eq!(
            seconds(Some(&cm(5, 1, flags))),
            None,
            "infinite or indefinite: {flags}"
        );
    }
    assert_eq!(seconds(Some(&cm(5, 0, 1))), None, "no timescale");
    assert_eq!(seconds(Some(&Value::Real(5.0))), None);
    assert_eq!(seconds(None), None);
}

#[test]
fn reads_what_the_queue_says_it_is_doing() {
    let playing = info(&[
        ("duration", cm(120, 1, 1)),
        ("position", cm(12_500, 1000, 1)),
        ("rate", 1.0.into()),
        ("readyToPlay", true.into()),
        ("playbackState", "playing".into()),
    ]);
    assert_eq!(
        read_queue_info(&playing, ITEM),
        PlaybackInfo {
            state: PlaybackState::Playing,
            position: 12.5,
            duration: Some(120.0),
        }
    );
    let with = |key: &str, value: Value| {
        let mut info = playing.clone();
        info.insert(key.into(), value);
        read_queue_info(&info, ITEM).state
    };
    assert_eq!(
        with("playbackState", "paused".into()),
        PlaybackState::Paused
    );
    assert_eq!(
        with("playbackState", "loading".into()),
        PlaybackState::Loading
    );
    assert_eq!(
        with("playbackBufferEmpty", true.into()),
        PlaybackState::Loading
    );
    assert_eq!(
        with("playbackBufferEmpty", false.into()),
        PlaybackState::Playing
    );
    assert_eq!(
        with("playbackState", "stopped".into()),
        PlaybackState::Ended
    );
    let mut at_end = playing.clone();
    at_end.insert("playbackState".into(), "paused".into());
    at_end.insert("position".into(), cm(119_600, 1000, 1));
    assert_eq!(read_queue_info(&at_end, ITEM).state, PlaybackState::Ended);
    at_end.insert("position".into(), cm(119_400, 1000, 1));
    assert_eq!(read_queue_info(&at_end, ITEM).state, PlaybackState::Paused);
}

#[test]
fn what_the_queue_wasnt_ready_to_play_is_loading_whatever_it_says() {
    // As when what it was given doesn't come: paused, and never ready.
    let waiting = info(&[
        ("readyToPlay", false.into()),
        ("playbackState", "paused".into()),
    ]);
    assert_eq!(
        read_queue_info(&waiting, ITEM).state,
        PlaybackState::Loading
    );
    let nothing = info(&[]);
    assert_eq!(
        read_queue_info(&nothing, ITEM),
        PlaybackInfo {
            state: PlaybackState::Loading,
            position: 0.0,
            duration: None,
        }
    );
    let ended = info(&[("playbackState", "stopped".into())]);
    assert_eq!(read_queue_info(&ended, ITEM).state, PlaybackState::Ended);
}

#[test]
fn what_the_queue_says_of_the_item_before_is_loading_the_one_given() {
    let of = |uuid: &str| {
        info(&[
            ("item", info(&[("uuid", uuid.into())]).into()),
            ("readyToPlay", true.into()),
            ("playbackState", "stopped".into()),
        ])
    };
    assert_eq!(read_queue_info(&of(ITEM), ITEM).state, PlaybackState::Ended);
    assert_eq!(
        read_queue_info(&of("1F0B7C55-2D1E-4E0A-9C11-0A5B3D2E4F60"), ITEM),
        PlaybackInfo {
            state: PlaybackState::Loading,
            position: 0.0,
            duration: None,
        }
    );
}
