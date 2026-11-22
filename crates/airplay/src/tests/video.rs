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
