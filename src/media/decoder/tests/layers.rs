use super::*;
use crate::media::tests::{Layout, profile_7_media};

/// A configuration record of profile 7 with an RPU, the enhancement layer
/// and the base layer as asked.
fn record(enhancement: bool, base: bool) -> [u8; 9] {
    [1, 0, 7, 6, 1, enhancement.into(), base.into(), 6, 0]
}

/// The parameters of a video stream of `codec`, with a configuration
/// record if given.
fn parameters(codec: codec::Id, record: Option<[u8; 9]>) -> codec::Parameters {
    let mut parameters = codec::Parameters::new();
    // SAFETY: the parameters are freshly allocated, and FFmpeg makes the
    // side data as big as asked, owned by them.
    unsafe {
        let raw = parameters.as_mut_ptr();
        (*raw).codec_type = ffi::AVMediaType::AVMEDIA_TYPE_VIDEO;
        (*raw).codec_id = codec.into();
        (*raw).width = 64;
        (*raw).height = 64;
        if let Some(record) = record {
            let side = ffi::av_packet_side_data_new(
                &mut (*raw).coded_side_data,
                &mut (*raw).nb_coded_side_data,
                ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
                record.len(),
                0,
            );
            assert!(!side.is_null());
            std::ptr::copy_nonoverlapping(record.as_ptr(), (*side).data, record.len());
        }
    }
    parameters
}

fn stream(index: usize, parameters: &codec::Parameters) -> Stream<'_> {
    Stream {
        index,
        parameters,
        time_base: Rational::new(1, 1000),
    }
}

/// The enhancement layer of a 24 frames a second video, in a track of its
/// own, with nothing decoded.
fn layers() -> Layers {
    let base = parameters(codec::Id::HEVC, None);
    let layer = parameters(codec::Id::HEVC, Some(record(true, false)));
    let streams = [stream(0, &base), stream(1, &layer)];
    Layers::of(streams[0], 1.0 / 24.0, &streams).unwrap()
}

/// A frame of nothing at `time`, as a decoder makes it.
fn at(time: Option<i64>) -> frame::Video {
    let mut frame = frame::Video::empty();
    // SAFETY: writing a field of a valid frame.
    unsafe { (*frame.as_mut_ptr()).best_effort_timestamp = time.unwrap_or(ffi::AV_NOPTS_VALUE) };
    frame
}

#[test]
fn finds_the_enhancement_layer_in_a_track_of_its_own_or_in_the_videos() {
    ffmpeg::init().unwrap();
    let base = parameters(codec::Id::HEVC, None);
    let other = parameters(codec::Id::HEVC, None);
    let layer = parameters(codec::Id::HEVC, Some(record(true, false)));
    let both = parameters(codec::Id::HEVC, Some(record(true, true)));
    let avc = parameters(codec::Id::H264, None);
    let avc_layer = parameters(codec::Id::H264, Some(record(true, false)));

    let streams = [stream(0, &base), stream(1, &other), stream(2, &layer)];
    let found = Layers::of(streams[0], 1.0 / 24.0, &streams).unwrap();
    assert_eq!(found.stream, Some(2));
    assert!(found.split.is_none());
    assert_eq!(found.tolerance, 20, "half a frame, in milliseconds");

    let found = Layers::of(stream(0, &both), 1.0 / 24.0, &[stream(0, &both)]).unwrap();
    assert_eq!(found.stream, None, "split out of the video's own track");
    assert!(found.split.is_some());

    for (what, base, streams) in [
        ("none", &base, vec![stream(0, &base), stream(1, &other)]),
        ("of H.264", &avc, vec![stream(0, &avc), stream(1, &layer)]),
        (
            "an H.264 one",
            &base,
            vec![stream(0, &base), stream(1, &avc_layer)],
        ),
        ("itself", &layer, vec![stream(0, &layer)]),
    ] {
        assert!(
            Layers::of(stream(0, base), 1.0 / 24.0, &streams).is_none(),
            "{what}"
        );
    }
}

#[test]
fn takes_the_frame_of_the_picture_asked_for() {
    ffmpeg::init().unwrap();
    let mut layers = layers();
    assert!(matches!(layers.take(1000), Found::Later), "nothing yet");
    for time in [
        None,
        Some(958),
        Some(979),
        Some(980),
        Some(1042),
        Some(1083),
    ] {
        layers.frames.push_back(at(time));
    }
    // Those before its time go: with none, or more than half a frame early.
    let Found::Frame(frame) = layers.take(1000) else {
        panic!("half a frame early is the same picture");
    };
    assert_eq!(frame.timestamp(), Some(980));
    // Half a frame late is too.
    let Found::Frame(frame) = layers.take(1022) else {
        panic!("half a frame late");
    };
    assert_eq!(frame.timestamp(), Some(1042));
    assert!(
        matches!(layers.take(1062), Found::Missing),
        "the next is of a later picture"
    );
    assert_eq!(layers.frames.len(), 1, "which stays for it");
    assert!(matches!(layers.take(1083), Found::Frame(_)));
    assert!(matches!(layers.take(1125), Found::Later));
    layers.finished = true;
    assert!(matches!(layers.take(1125), Found::Missing), "at the end");
    layers.frames.push_back(at(Some(1125)));
    layers.reset();
    assert!(layers.frames.is_empty(), "a seek leaves none");
    assert!(matches!(layers.take(1125), Found::Later));
}

#[test]
fn keeps_so_many_frames_ahead() {
    ffmpeg::init().unwrap();
    let mut layers = layers();
    for time in 0..=MOST_AHEAD as i64 {
        layers.keep(at(Some(time)));
    }
    assert_eq!(layers.frames.len(), MOST_AHEAD);
    assert_eq!(layers.frames[0].timestamp(), Some(1), "the oldest went");
}

/// The packets of the first track of the profile 7 file laid out so, and
/// that track's parameters and time base.
fn packets(layout: Layout) -> (Vec<Packet>, codec::Parameters, Rational) {
    ffmpeg::init().unwrap();
    let mut input = ffmpeg::format::input(&profile_7_media(layout)).unwrap();
    let (parameters, time_base) = {
        let stream = input.stream(0).unwrap();
        (stream.parameters(), stream.time_base())
    };
    let packets = input
        .packets()
        .filter(|(stream, _)| stream.index() == 0)
        .map(|(_, packet)| packet)
        .collect();
    (packets, parameters, time_base)
}

/// Each frame of `packets`, decoded into `layers`, found by its time, and
/// none after the last; and none once reset.
fn finds_each_frame(mut layers: Layers, packets: &[Packet]) {
    let times: Vec<i64> = packets.iter().filter_map(Packet::pts).collect();
    assert_eq!(times.len(), 10);
    for packet in packets {
        layers.decode(packet);
    }
    layers.finish();
    for time in &times {
        let Found::Frame(frame) = layers.take(*time) else {
            panic!("the frame at {time} of {times:?}");
        };
        assert_eq!((frame.width(), frame.height()), (320, 180));
    }
    assert!(matches!(layers.take(times[9] + 1000), Found::Missing));
    layers.reset();
    assert!(!layers.finished);
    assert!(matches!(layers.take(0), Found::Later));
}

#[test]
fn decodes_a_track_of_its_own_and_finds_each_frame_by_its_time() {
    let base_parameters = parameters(codec::Id::HEVC, None);
    let (packets, parameters, time_base) = packets(Layout::TwoTracks);
    let layer = Stream {
        index: 0,
        parameters: &parameters,
        time_base,
    };
    let base = stream(1, &base_parameters);
    let layers = Layers::of(base, 0.1, &[layer, base]).unwrap();
    assert_eq!(layers.stream, Some(0));
    finds_each_frame(layers, &packets);
}

#[test]
fn splits_the_enhancement_layer_out_of_the_videos_track() {
    let (packets, parameters, time_base) = packets(Layout::OneTrack);
    let video = Stream {
        index: 0,
        parameters: &parameters,
        time_base,
    };
    let layers = Layers::of(video, 0.1, &[video]).unwrap();
    assert!(layers.split.is_some());
    finds_each_frame(layers, &packets);
}

#[test]
fn a_track_with_no_enhancement_layer_in_it_splits_into_nothing() {
    let (packets, parameters, time_base) = packets(Layout::Missing);
    let video = Stream {
        index: 0,
        parameters: &parameters,
        time_base,
    };
    let mut layers = Layers::of(video, 0.1, &[video]).unwrap();
    for packet in &packets {
        layers.decode(packet);
    }
    assert!(matches!(layers.take(0), Found::Later));
    layers.finish();
    assert!(layers.frames.is_empty());
    assert!(matches!(layers.take(0), Found::Missing));
    layers.reset();
    assert!(matches!(layers.take(0), Found::Later));
}
