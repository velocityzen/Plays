use std::net::SocketAddr;

use super::*;
use crate::cast::fake::{Call, FakeTarget};
use crate::cast::target::Shows;
use crate::ui::testing::fixture;

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

/// Sent back from the end and played at once, the two in one go before
/// the device says where it is, it plays from where it was sent, as it
/// does once the device has said: it isn't played again from the start.
#[test]
fn played_right_after_being_sent_back_from_the_end_it_plays_from_there() {
    let fake = FakeTarget::default();
    let device = fake.add("TV", Shows::Video);
    let server = Server::start_on(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
    let request = Request {
        source: Source::File(fixture("clip.mp4")),
        title: "clip.mp4".into(),
        video: Pick::Best,
        audio: Pick::Best,
        subtitles: None,
        start: 0.0,
        autoplay: true,
        volume: 1.0,
        muted: false,
        looping: false,
        picture_here: false,
    };
    let shared = Arc::new(Mutex::new(Shared {
        phase: Phase::Connecting,
        status: None,
        moving: false,
        done: 0,
        duration: None,
        failure: None,
    }));
    let mut worker = Worker::start(&fake, &device, Arc::new(server), request, shared).unwrap();
    // As heard at the end.
    worker.state = State::Ended;

    let (commands, received) = mpsc::channel();
    commands.send(Command::Seek(1.0)).unwrap();
    commands.send(Command::Play).unwrap();
    drop(commands);
    worker.serve(received);
    let calls = fake.screen(&device).lock().calls.clone();
    assert!(matches!(calls[0], Call::Load(_)), "{calls:?}");
    assert_eq!(calls[1..], [Call::Seek(1.0), Call::Play, Call::Stop]);
}
