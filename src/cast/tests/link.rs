use std::net::SocketAddr;

use super::*;
use stream_server::Profile;

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
fn keeps_only_the_last_volume_looping_subtitles_and_changes_asked_for() {
    let volume = |volume, muted| Command::Volume { volume, muted };
    let subtitles = |start| Command::Subtitles {
        subtitles: None,
        start,
        autoplay: true,
    };
    let changes = |audio_delay| Command::Changes {
        changes: Changes {
            audio_delay,
            ..Changes::default()
        },
        start: 0.0,
        autoplay: true,
    };
    // As dragging the volume slider sends them, with other things.
    let kept = coalesce(vec![
        volume(0.1, false),
        Command::Looping(true),
        changes(0.1),
        volume(0.2, false),
        Command::Seek(1.0),
        subtitles(1.0),
        volume(0.3, false),
        Command::Seek(2.0),
        Command::Play,
        changes(0.2),
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
            Command::Changes { changes, .. } => format!("delay {}", changes.audio_delay),
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
            "delay 0.2",
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
    let (mut worker, _) = worker(&fake, &device);
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

/// The clip cast to `device`, as the link's thread casts it, and what the
/// thread tells the block.
fn worker(fake: &FakeTarget, device: &Device) -> (Worker, Arc<Mutex<Shared>>) {
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
        changes: Changes::default(),
    };
    let shared = Arc::new(Mutex::new(Shared {
        phase: Phase::Connecting,
        status: None,
        moving: false,
        done: 0,
        duration: None,
        failure: None,
    }));
    let worker = Worker::start(fake, device, Arc::new(server), request, Arc::clone(&shared));
    (worker.unwrap(), shared)
}

/// Where each time `device` was told to load the media had it start.
fn loaded_from(fake: &FakeTarget, device: &Device) -> Vec<f64> {
    let screen = fake.screen(device);
    let calls = &screen.lock().calls;
    calls
        .iter()
        .filter_map(|call| match call {
            Call::Load(load) => Some(load.start),
            _ => None,
        })
        .collect()
}

fn changes(changes: Changes) -> Command {
    Command::Changes {
        changes,
        start: 1.0,
        autoplay: true,
    }
}

/// The clip, sent as it is, is served again from where it was to delay
/// its sound or subtitles; asked for what it's served with already, it's
/// left as it is.
#[test]
fn serves_the_media_again_for_other_delays() {
    let fake = FakeTarget::default();
    let device = fake.add("TV", Shows::Video);
    let (mut worker, shared) = worker(&fake, &device);
    worker.apply(changes(Changes::default())).unwrap();
    assert_eq!(loaded_from(&fake, &device), [0.0], "as it was");

    let delayed = Changes {
        audio_delay: 0.5,
        ..Changes::default()
    };
    worker.apply(changes(delayed)).unwrap();
    assert_eq!(loaded_from(&fake, &device), [0.0, 1.0]);
    assert_eq!(worker.cast.plan().audio_delay, 0.5);
    assert!(matches!(
        shared.lock().phase,
        Phase::On {
            delays_sound: true,
            adjusts_picture: false,
            ..
        }
    ));
    worker.apply(changes(delayed)).unwrap();
    assert_eq!(loaded_from(&fake, &device), [0.0, 1.0], "delayed already");

    worker
        .apply(changes(Changes {
            subtitle_delay: -2.0,
            ..delayed
        }))
        .unwrap();
    assert_eq!(loaded_from(&fake, &device), [0.0, 1.0, 1.0]);
    assert_eq!(worker.cast.request().subtitle_delay, -2.0);
}

/// A device that takes only files gets the media as it is: asked to
/// adjust its picture or delay its sound, it's left so, what was asked
/// kept for when it's served again.
#[test]
fn leaves_media_it_cant_change_as_it_is() {
    let fake = FakeTarget::default();
    let device = fake.add("TV", Shows::Video);
    fake.screen(&device).lock().profile = Profile {
        hls: None,
        ..Profile::default()
    };
    let (mut worker, shared) = worker(&fake, &device);
    let asked = Changes {
        audio_delay: 0.5,
        subtitle_delay: 0.0,
        adjustments: Some(Adjustments {
            turns: 1,
            ..Adjustments::default()
        }),
    };
    worker.apply(changes(asked)).unwrap();
    assert_eq!(loaded_from(&fake, &device), [0.0]);
    assert!(matches!(
        shared.lock().phase,
        Phase::On {
            delays_sound: false,
            adjusts_picture: false,
            ..
        }
    ));
    assert_eq!(worker.request.audio_delay, 0.5);
    assert_eq!(worker.request.adjustments, asked.adjustments);
}
