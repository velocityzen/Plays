use std::path::PathBuf;

use super::*;
use crate::plan::{Pick, plan};
use crate::profile::{AudioCodec, Profile};

/// The tone, re-encoded to AC-3, in segments a tenth of a second long:
/// ten of them.
fn tone() -> Arc<Vod> {
    let (media, timeline) = tone_in(None);
    Vod::new(Arc::new(media), timeline)
}

/// The tone as AC-3 in `hls` segments (the profile's own by default), and
/// its ten segments.
fn tone_in(hls: Option<Segments>) -> (Media, Timeline) {
    crate::init();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tone.m4a");
    let source = Source::File(path);
    let mut input = media::open(&source, Arc::default()).unwrap();
    let info = MediaInfo::read(&input, &source.location());
    let mut profile = Profile {
        files: Vec::new(),
        audio: vec![AudioCodec::Ac3],
        ..Profile::default()
    };
    profile.hls = hls.or(profile.hls);
    let plan = plan(&info, &source, Pick::Best, Pick::Best, None, &profile).unwrap();
    let timeline = crate::segments::timeline(&mut input, &info, &plan, 0.1);
    assert_eq!(timeline.len(), 10);
    let media = Media {
        source,
        kind: plan.segments(),
        info,
        plan,
        segment: 0.1,
    };
    (media, timeline)
}

/// A job from the start and one from the last segment, which only ends:
/// each hands over MP4's init segment first, once, as it cuts or ends the
/// segment the init segment was written with.
#[test]
fn hands_over_mp4s_init_segment_before_its_first_segment() {
    let (media, timeline) = tone_in(Some(Segments::Fmp4));
    assert_eq!(media.kind, Segments::Fmp4);
    for from in [0, 9] {
        let mut made = Vec::new();
        make_segments(&media, &timeline, from, &Arc::default(), &mut |product| {
            made.push(match product {
                Product::Init(_) => None,
                Product::Segment(segment, _) => Some(segment),
            });
            true
        })
        .unwrap();
        assert_eq!(made[..2], [None, Some(from)], "from {from}");
        assert_eq!(made.iter().filter(|made| made.is_none()).count(), 1);
    }
}

/// A live stream cut into segments, and one that ends before its first
/// cut: each hands over MP4's init segment first, once.
#[test]
fn a_live_stream_hands_over_mp4s_init_segment_before_its_first_segment() {
    let (mut media, _) = tone_in(Some(Segments::Fmp4));
    for segment in [0.1, 10.0] {
        media.segment = segment;
        let mut made = Vec::new();
        make_live(&media, &Arc::default(), &mut |product| {
            made.push(matches!(product, LiveProduct::Init(_)));
            true
        })
        .unwrap();
        assert!(made.len() >= 2, "{segment} s segments: {made:?}");
        assert!(made[0], "{segment} s segments start with the init segment");
        assert_eq!(made.iter().filter(|init| **init).count(), 1);
    }
}

#[test]
fn its_playlists_are_made_once() {
    let vod = tone();
    let playlist = vod.playlist();
    assert!(playlist.ends_with(b"9.ts\n#EXT-X-ENDLIST\n"));
    assert!(Arc::ptr_eq(&playlist, &vod.playlist()), "made again");
    let subtitles = vod.subtitles_playlist();
    assert!(subtitles.ends_with(b"s9.vtt\n#EXT-X-ENDLIST\n"));
    assert!(
        Arc::ptr_eq(&subtitles, &vod.subtitles_playlist()),
        "made again"
    );
}

/// The segments kept once the job has stopped for now, and where the
/// job started.
fn settled(vod: &Vod) -> (Vec<usize>, Option<usize>) {
    let mut last = None;
    for _ in 0..200 {
        let state = lock(&vod.state);
        let now = (
            state.segments.keys().copied().collect::<Vec<_>>(),
            state.job.as_ref().map(|job| (job.from, job.next)),
        );
        drop(state);
        if last.as_ref() == Some(&now) {
            return (now.0, now.1.map(|(from, _)| from));
        }
        last = Some(now);
        thread::sleep(Duration::from_millis(20));
    }
    panic!("the job never settled");
}

fn packet(output: usize, dts: i64, duration: i64) -> Out {
    let mut packet = Packet::empty();
    packet.set_dts(Some(dts));
    packet.set_pts(Some(dts));
    packet.set_duration(duration);
    Out {
        packet,
        output,
        time: 0.0,
        key: false,
        mark: None,
    }
}

#[test]
fn gives_packets_their_lengths_from_the_next() {
    let mut lengths = Lengths::new(2);
    let mut passed = Vec::new();
    for out in [
        packet(0, 0, 0),
        packet(1, 0, 1024),
        packet(0, 40, 0),
        packet(0, 80, 0),
        packet(1, 1024, 1024),
        packet(0, 120, 30),
    ] {
        passed.extend(lengths.pass(out));
    }
    passed.extend(lengths.rest());
    let lengths: Vec<(usize, i64, i64)> = passed
        .iter()
        .map(|out| (out.output, out.packet.dts().unwrap(), out.packet.duration()))
        .collect();
    assert_eq!(
        lengths,
        [
            (0, 0, 40),
            (0, 40, 40),
            (1, 0, 1024),
            (0, 80, 40),
            // Its own length, and the stream's last one.
            (0, 120, 30),
            (1, 1024, 1024),
        ]
    );
    // A last packet with none takes the one before's.
    let mut alone = Lengths::new(1);
    assert!(alone.pass(packet(0, 0, 0)).is_none());
    assert_eq!(alone.pass(packet(0, 50, 0)).unwrap().packet.duration(), 50);
    assert_eq!(alone.rest()[0].packet.duration(), 50);
}

#[test]
fn going_back_past_what_it_kept_while_it_runs_starts_a_job_there() {
    let vod = tone();
    vod.segment(0).unwrap();
    vod.segment(2).unwrap();
    vod.segment(5).unwrap();
    let (kept, from) = settled(&vod);
    assert_eq!(from, Some(0), "the job from the start still runs");
    assert!(!kept.contains(&0), "and has forgotten the first: {kept:?}");
    let asked = Instant::now();
    assert!(!vod.segment(0).unwrap().is_empty());
    assert!(
        asked.elapsed() < Duration::from_secs(10),
        "made again in {:?}",
        asked.elapsed()
    );
    vod.close();
}

#[test]
fn makes_a_few_segments_ahead_and_restarts_far_away() {
    let vod = tone();
    assert!(!vod.segment(0).unwrap().is_empty());
    // Three ahead of the one asked for, then it waits.
    assert_eq!(settled(&vod), (vec![0, 1, 2, 3], Some(0)));
    // Asking for one it has lets it go on.
    vod.segment(2).unwrap();
    assert_eq!(settled(&vod), (vec![0, 1, 2, 3, 4, 5], Some(0)));
    // One a little ahead is waited for, by the same job.
    vod.segment(7).unwrap();
    let (kept, from) = settled(&vod);
    assert_eq!(from, None, "it reached the end");
    assert_eq!(kept, [3, 4, 5, 6, 7, 8, 9]);
    // Going back starts a job there, and forgets what is far from it.
    vod.segment(1).unwrap();
    let (kept, from) = settled(&vod);
    assert_eq!(from, Some(1));
    assert_eq!(kept, [1, 2, 3, 4, 5, 6, 7]);
    vod.close();
    assert!(vod.segment(1).is_err());
    assert!(lock(&vod.state).job.is_none());
}
