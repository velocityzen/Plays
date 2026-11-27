use super::*;
use crate::media::decoder::MediaInfo;
use crate::media::decoder::shared::Shared;
use crate::media::tests::made::media;

/// Ten frames of AV1, one keyframe, as SVT-AV1 encodes them.
fn av1_media() -> std::path::PathBuf {
    media(
        "ten-frames-av1.mp4",
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x180:rate=10:duration=1",
            "-c:v",
            "libsvtav1",
            "-preset",
            "12",
            "-g",
            "10",
            "-pix_fmt",
            "yuv420p",
        ],
    )
}

/// The video stream of `path` and its packets.
fn video(path: &std::path::Path) -> (StreamInfo, Vec<Packet>) {
    let mut input = ffmpeg_safe::input_file(path).unwrap();
    let stream = StreamInfo::of(&input.streams().best(ffmpeg::media::Type::Video).unwrap());
    let packets = input
        .packets()
        .filter(|(s, _)| s.index() == stream.index)
        .map(|(_, packet)| packet)
        .collect();
    (stream, packets)
}

fn session(shared: &Shared) -> Session<'_> {
    let serial = shared.serial();
    shared.reset_queues(serial);
    Session {
        shared,
        name: "test",
        serial,
        seek_target: None,
        settled: true,
        scrub: false,
        start_offset: None,
    }
}

/// The times of the frames `decoder` shows of `packets`, to the end, in
/// milliseconds, and the API the media info says decodes them.
fn shown(decoder: &mut VideoDecoder, packets: &[Packet]) -> (Vec<i64>, Option<&'static str>) {
    let shared = Shared::new(None, 1.0);
    shared.set_info(MediaInfo::default());
    let session = session(&shared);
    for packet in packets {
        decoder.decode(packet, &session);
    }
    decoder.finish(&session);
    let frames = shared.frames().unwrap();
    let times = frames
        .iter()
        .map(|frame| (frame.pts * 1000.0).round() as i64)
        .collect();
    (times, shared.info().unwrap().hardware)
}

/// Every tenth of a second, from 0, in milliseconds.
fn tenths(count: i64) -> Vec<i64> {
    (0..count).map(|i| i * 100).collect()
}

#[test]
fn a_picture_past_4k_takes_more_threads_to_decode() {
    assert_eq!(threads(16, 1920 * 1080), 6);
    assert_eq!(threads(16, 3840 * 2160), 6);
    assert_eq!(threads(16, 3840 * 2160 + 1), 7);
    assert_eq!(threads(16, 7680 * 4320), 16);
    assert_eq!(threads(64, 7680 * 4320), 24);
    // Never fewer than the few a small one takes.
    assert_eq!(threads(4, 7680 * 4320), 4);
    assert_eq!(threads(1, 7680 * 4320), 2);
    assert_eq!(threads(16, 0), 6);
}

#[test]
fn a_decoder_takes_threads_for_the_size_of_its_pictures() {
    let cores = thread::available_parallelism().map_or(4, NonZero::get);
    for (width, height) in [(1920, 1080), (7680, 4320)] {
        let mut parameters = codec::Parameters::new();
        // SAFETY: the parameters are freshly allocated.
        unsafe {
            let raw = parameters.as_mut_ptr();
            (*raw).codec_type = ffi::AVMediaType::AVMEDIA_TYPE_VIDEO;
            (*raw).codec_id = codec::Id::HEVC.into();
            (*raw).width = width;
            (*raw).height = height;
        }
        let (decoder, _) = open_video(&parameters, false).unwrap();
        // SAFETY: reading the open decoder's thread count.
        let count = unsafe { (*decoder.as_ptr()).thread_count };
        let pixels = (width * height) as usize;
        assert_eq!(count as usize, threads(cores, pixels), "{width} x {height}");
    }
}

#[test]
fn av1_decodes_on_the_gpu_where_it_can() {
    let (stream, packets) = video(&av1_media());
    let default = ffmpeg::decoder::find(codec::Id::AV1).unwrap();
    let gpu = hwaccel::instead_of(&default).is_some();
    let mut decoder = VideoDecoder::new(&stream, None, true).unwrap();
    assert_eq!(
        decoder.hardware.map(|hardware| hardware.only),
        gpu.then_some(true)
    );
    assert_eq!(
        decoder.decoder.codec().unwrap().name(),
        if gpu { "av1" } else { "libdav1d" }
    );
    assert_eq!(
        shown(&mut decoder, &packets),
        (tenths(10), gpu.then_some("VideoToolbox"))
    );
    assert_eq!(decoder.reported, Some(gpu), "where its frames were");
    assert_eq!(decoder.hardware.is_some(), gpu);

    // Not asked to, it doesn't.
    let decoder = VideoDecoder::new(&stream, None, false).unwrap();
    assert_eq!(decoder.decoder.codec().unwrap().name(), "libdav1d");
    assert_eq!(decoder.hardware, None);
}

/// A decoder of `stream` that decodes only on a GPU it doesn't have:
/// FFmpeg's own AV1 decoder, given none.
fn without_its_gpu(stream: &StreamInfo, only: bool) -> VideoDecoder {
    let mut decoder = VideoDecoder::new(stream, None, false).unwrap();
    let context = codec::context::Context::from_parameters(stream.parameters.clone()).unwrap();
    let own = ffmpeg::decoder::find_by_name("av1").unwrap();
    decoder.decoder = context.decoder().open_as(own).unwrap().video().unwrap();
    decoder.hardware = Some(Hardware {
        api: "VideoToolbox",
        only,
    });
    decoder
}

#[test]
fn a_decoder_only_the_gpu_decodes_on_goes_to_software_at_its_first_error() {
    let (stream, packets) = video(&av1_media());
    let mut decoder = without_its_gpu(&stream, true);
    assert_eq!(
        shown(&mut decoder, &packets),
        (tenths(10), None),
        "the first frame too, from the CPU"
    );
    assert_eq!(decoder.hardware, None);
    assert_eq!(decoder.decoder.codec().unwrap().name(), "libdav1d");

    // One that decodes in software too gets a few tries, the frames up to
    // the next keyframe lost meanwhile.
    let mut decoder = without_its_gpu(&stream, false);
    assert_eq!(shown(&mut decoder, &packets), (Vec::new(), None));
    assert!(
        (1..HARDWARE_ERROR_LIMIT).contains(&decoder.errors),
        "{}",
        decoder.errors
    );
}

#[test]
fn frames_the_gpu_leaves_to_the_cpu_are_said_to_be_decoded_in_software() {
    let (stream, packets) = video(&av1_media());
    let mut decoder = VideoDecoder::new(&stream, None, false).unwrap();
    decoder.hardware = Some(Hardware {
        api: "VideoToolbox",
        only: false,
    });
    assert_eq!(shown(&mut decoder, &packets), (tenths(10), None));
    assert_eq!(decoder.reported, Some(false));
}

#[test]
fn hevc_of_two_layers_decodes_its_base_layer_on_the_gpu() {
    use crate::media::decoder::base_layer::tests::two_layer_media;
    use crate::media::{still, tests::first_frame};

    let path = two_layer_media();
    let (stream, packets) = video(&path);
    let mut decoder = VideoDecoder::new(&stream, None, true).unwrap();
    let gpu = decoder.hardware.is_some();
    assert_eq!(
        decoder.base.is_some(),
        gpu,
        "packets go as they are to the CPU"
    );
    assert_eq!(
        shown(&mut decoder, &packets),
        (tenths(10), gpu.then_some("VideoToolbox"))
    );
    let mut decoder = VideoDecoder::new(&stream, None, false).unwrap();
    assert!(decoder.base.is_none());
    assert_eq!(shown(&mut decoder, &packets), (tenths(10), None));

    // The same picture either way.
    let [gpu, cpu] =
        [true, false].map(|hardware| still::shown(&first_frame(&path, hardware), false).unwrap());
    assert_eq!((gpu.width, gpu.height), (cpu.width, cpu.height));
    let apart = gpu
        .pixels
        .iter()
        .zip(&cpu.pixels)
        .flat_map(|(a, b)| {
            a.iter()
                .zip(b)
                .map(|(a, b)| f64::from(a.abs_diff(*b)) / 257.0)
        })
        .sum::<f64>()
        / (gpu.pixels.len() * 3) as f64;
    assert!(apart < 2.0, "{apart:.2} levels apart on average");
}
