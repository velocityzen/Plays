use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::media::{self, Source};
use crate::plan::Delivery;
use crate::profile::{Segments, VideoCodec};

fn opened(name: &str) -> (Input, MediaInfo) {
    crate::init();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    let source = Source::File(path);
    let input = media::open(&source, Arc::default()).expect("the fixture");
    let info = MediaInfo::read(&input, &source.location());
    (input, info)
}

#[test]
fn a_file_claiming_to_last_for_ever_has_no_more_than_the_most_segments() {
    let target = 6.0;
    let most = MOST_SEGMENTS as f64 * target;
    let sound = Plan {
        delivery: Delivery::Hls(Segments::Ts),
        video: None,
        audio: None,
        subtitles: None,
        audio_delay: 0.0,
        adjustments: None,
    };
    let copied = Plan {
        video: Some(VideoPlan::Copy {
            stream: 0,
            codec: VideoCodec::H264,
        }),
        ..sound.clone()
    };
    // Cut every target length, and at the video's keyframes.
    for (name, plan) in [("clip.mp4", copied), ("tone.m4a", sound)] {
        let (mut input, mut info) = opened(name);
        info.duration = Some(10.0 * most);
        let cut = timeline(&mut input, &info, &plan, target);
        assert!(cut.len() <= MOST_SEGMENTS, "{name}: {} segments", cut.len());
        assert!(cut.end - info.start <= most, "{name}: ends at {}", cut.end);
    }
}
