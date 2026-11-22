use super::*;

#[test]
fn keeps_only_the_last_of_seeks_in_a_row() {
    let kept = coalesce(vec![
        Command::Seek(1.0),
        Command::Seek(2.0),
        Command::Pause,
        Command::Seek(3.0),
        Command::Seek(4.0),
    ]);
    let seeks: Vec<Option<f64>> = kept
        .iter()
        .map(|command| match command {
            Command::Seek(time) => Some(*time),
            _ => None,
        })
        .collect();
    assert_eq!(seeks, [Some(2.0), None, Some(4.0)]);
}

#[test]
fn keeps_only_the_last_volume_looping_and_subtitles_asked_for() {
    let volume = |volume, muted| Command::Volume { volume, muted };
    let subtitles = |start| Command::Subtitles {
        subtitles: None,
        start,
        autoplay: true,
    };
    // As dragging the volume slider sends them, with other things.
    let kept = coalesce(vec![
        volume(0.1, false),
        Command::Looping(true),
        volume(0.2, false),
        Command::Seek(1.0),
        subtitles(1.0),
        volume(0.3, false),
        Command::Seek(2.0),
        Command::Play,
        volume(0.4, true),
        Command::Looping(false),
        subtitles(2.0),
    ]);
    let said: Vec<String> = kept
        .iter()
        .map(|command| match command {
            Command::Volume { volume, muted } => format!("volume {volume} {muted}"),
            Command::Looping(looping) => format!("looping {looping}"),
            Command::Subtitles { start, .. } => format!("subtitles from {start}"),
            Command::Seek(time) => format!("seek {time}"),
            Command::Play => "play".into(),
            _ => "other".into(),
        })
        .collect();
    assert_eq!(
        said,
        [
            "seek 2",
            "play",
            "volume 0.4 true",
            "looping false",
            "subtitles from 2"
        ]
    );
}
