use super::*;
use crate::media::audio::AudioChunk;

const RATE: u32 = 48_000;

/// A frame for the seek `serial`, its picture a single pixel.
fn frame(serial: u64, pts: f64) -> VideoFrame {
    let pixel = image::RgbaImage::new(1, 1);
    let image = RenderImage::new(vec![image::Frame::new(pixel)]);
    VideoFrame {
        pts,
        timestamp: None,
        serial,
        picture: Picture::Image(Arc::new(image)),
    }
}

/// The times of the frames handed out now.
fn handed_out(shared: &Shared) -> Option<Vec<f64>> {
    let frames = shared.frames()?;
    Some(frames.iter().map(|frame| frame.pts).collect())
}

#[test]
fn a_frame_queued_before_a_seek_is_never_handed_out_after_it() {
    let shared = Shared::new(None, 1.0);
    assert_eq!(handed_out(&shared), None, "nothing decoded yet");
    let first = shared.serial();
    shared.reset_queues(first);
    shared.push_frame(frame(first, 0.0));
    assert_eq!(handed_out(&shared), Some(vec![0.0]));

    let second = shared.new_serial();
    assert_eq!(
        handed_out(&shared),
        None,
        "the decode thread hasn't got to the seek yet"
    );
    assert_eq!(shared.queued_frames(), 1, "the decode thread's own");

    shared.reset_queues(second);
    // One decoded for the seek before arriving late.
    shared.push_frame(frame(first, 1.0));
    shared.push_frame(frame(second, 2.0));
    assert_eq!(handed_out(&shared), Some(vec![2.0]));
}

#[test]
fn sound_queued_before_a_seek_is_not_counted_after_it() {
    let shared = Shared::new(Some(RATE), 1.0);
    let audio = shared.audio().expect("a mixer's rate was given");
    shared.reset_queues(shared.serial());
    audio.push(AudioChunk {
        pts: 0.0,
        samples: vec![0.0; RATE as usize * 2],
        pos: 0,
        rate: RATE,
    });
    assert_eq!(shared.queued_audio(), 1.0);

    shared.new_serial();
    assert_eq!(shared.queued_audio(), 0.0);
}

#[test]
fn the_sound_has_the_same_serial() {
    let shared = Shared::new(Some(RATE), 1.0);
    let serial = shared.new_serial();
    let audio = shared.audio().expect("a mixer's rate was given");
    assert_eq!(audio.serial.load(Ordering::Acquire), serial);
}

#[test]
fn a_landing_for_an_earlier_seek_is_not_taken() {
    let shared = Shared::new(None, 1.0);
    let first = shared.serial();
    shared.set_landed(first, 3.0);
    let second = shared.new_serial();
    assert_eq!(shared.take_landed(), None);

    shared.set_landed(second, 5.0);
    assert_eq!(shared.take_landed(), Some(5.0));
    assert_eq!(shared.take_landed(), None, "it is taken once");
}

#[test]
fn the_end_of_an_earlier_seek_is_not_the_end() {
    let shared = Shared::new(None, 1.0);
    assert!(!shared.eof(), "nothing decoded yet");
    let first = shared.serial();
    shared.set_eof(first);
    assert!(shared.eof());

    let second = shared.new_serial();
    assert!(!shared.eof(), "the seek after it has just started");
    shared.set_eof(second);
    assert!(shared.eof());
}

#[test]
fn the_first_failure_is_the_one_kept() {
    let shared = Shared::new(None, 1.0);
    assert_eq!(shared.error(), None);
    shared.fail("the demuxer failed".into());
    shared.fail("the decoder failed after it".into());
    assert_eq!(shared.error().as_deref(), Some("the demuxer failed"));
}

/// Subtitles picked and then none, quickly, twice: the decode thread,
/// behind, says what played for each of the first picks only after the
/// last were made. What it says of an earlier pick doesn't hide a later
/// one, so the last, no subtitles, which it said played a moment before,
/// is still sent, and shows.
#[test]
fn what_the_decode_thread_says_of_an_earlier_pick_doesnt_hide_a_later_one() {
    let shared = Shared::new(None, 1.0);
    shared.set_info(MediaInfo::default());
    let kind = TrackKind::Subtitle;
    let shown = || shared.info().unwrap().selected(kind);
    let picked = shared.select_track(kind, Some(2)).expect("a change");
    let none = shared.select_track(kind, None).expect("a change");
    let again = shared.select_track(kind, Some(2)).expect("a change");

    shared.played(kind, Some(2), picked);
    assert_eq!(shown(), Some(2));
    shared.played(kind, None, none);
    assert_eq!(shown(), Some(2), "as last picked");
    let last = shared
        .select_track(kind, None)
        .expect("sent, though none played a moment before");
    shared.played(kind, Some(2), again);
    assert_eq!(shown(), None, "as last picked");
    shared.played(kind, None, last);
    assert_eq!(shown(), None);
    assert_eq!(shared.select_track(kind, None), None, "no change");
}

/// Once it has done the last pick of a kind, it says what plays, a track
/// that couldn't be played put back; the picks of other kinds don't wait
/// on it, nor it on them.
#[test]
fn the_decode_thread_says_what_plays_once_it_has_done_the_last_pick() {
    let shared = Shared::new(None, 1.0);
    shared.set_info(MediaInfo::default());
    let audio = shared.select_track(TrackKind::Audio, Some(3)).unwrap();
    shared.select_track(TrackKind::Subtitle, Some(4)).unwrap();
    shared.played(TrackKind::Audio, Some(1), audio);
    let info = shared.info().unwrap();
    assert_eq!(info.selected(TrackKind::Audio), Some(1), "3 can't play");
    assert_eq!(info.selected(TrackKind::Subtitle), Some(4));
}
