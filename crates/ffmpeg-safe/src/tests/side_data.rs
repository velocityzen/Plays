use std::mem::offset_of;
use std::ptr;

use ffmpeg_next::format::Pixel;
use ffmpeg_next::{codec, ffi};

// `frame` is both the function and, as the module imports it, ffmpeg-next's
// module.
use super::{DisplayMatrix, DolbyVision, coded, frame, has_coded, rpu, sys};

#[test]
fn mirrors_ffmpegs_layout() {
    // From offsetof() against libavutil/mastering_display_metadata.h and
    // libavutil/dovi_meta.h.
    assert_eq!(offset_of!(sys::MasteringDisplay, white_point), 48);
    assert_eq!(offset_of!(sys::MasteringDisplay, max_luminance), 72);
    assert_eq!(offset_of!(sys::MasteringDisplay, has_luminance), 84);
    assert_eq!(size_of::<sys::MasteringDisplay>(), 88);
    assert_eq!(offset_of!(sys::ContentLightLevel, max_fall), 4);
    assert_eq!(size_of::<sys::ContentLightLevel>(), 8);
    assert_eq!(offset_of!(sys::DoviConfiguration, dv_profile), 2);
    assert_eq!(offset_of!(sys::DoviConfiguration, rpu_present_flag), 4);
    assert_eq!(offset_of!(sys::DoviConfiguration, el_present_flag), 5);
    assert_eq!(offset_of!(sys::DoviConfiguration, bl_present_flag), 6);
    assert_eq!(
        offset_of!(sys::DoviConfiguration, dv_bl_signal_compatibility_id),
        7
    );
    assert_eq!(size_of::<sys::DoviConfiguration>(), 9);
    assert_eq!(offset_of!(sys::RpuDataHeader, vdr_rpu_profile), 4);
    assert_eq!(offset_of!(sys::RpuDataHeader, coef_log2_denom), 8);
    assert_eq!(offset_of!(sys::RpuDataHeader, bl_bit_depth), 11);
    assert_eq!(offset_of!(sys::RpuDataHeader, disable_residual_flag), 16);
    assert_eq!(size_of::<sys::ReshapingCurve>(), 1672);
    assert_eq!(offset_of!(sys::ReshapingCurve, mapping_idc), 20);
    assert_eq!(offset_of!(sys::ReshapingCurve, poly_coef), 64);
    assert_eq!(offset_of!(sys::ReshapingCurve, mmr_constant), 264);
    assert_eq!(offset_of!(sys::ReshapingCurve, mmr_coef), 328);
    assert_eq!(offset_of!(sys::DataMapping, curves), 8);
    assert_eq!(offset_of!(sys::DataMapping, nlq_method_idc), 5024);
    assert_eq!(offset_of!(sys::DataMapping, num_x_partitions), 5028);
    assert_eq!(offset_of!(sys::DataMapping, num_y_partitions), 5032);
    assert_eq!(offset_of!(sys::DataMapping, nlq), 5040);
    assert_eq!(offset_of!(sys::DataMapping, nlq_pivots), 5136);
    assert_eq!(size_of::<sys::DataMapping>(), 5144);
    assert_eq!(size_of::<sys::NlqParams>(), 32);
    assert_eq!(offset_of!(sys::NlqParams, vdr_in_max), 8);
    assert_eq!(offset_of!(sys::NlqParams, linear_deadzone_threshold), 24);
    assert_eq!(offset_of!(sys::DoviMetadata, mapping_offset), 8);
    assert_eq!(offset_of!(sys::DoviMetadata, color_offset), 16);
    assert_eq!(offset_of!(sys::ColorMetadata, ycc_to_rgb_matrix), 4);
    assert_eq!(offset_of!(sys::ColorMetadata, ycc_to_rgb_offset), 76);
    assert_eq!(offset_of!(sys::ColorMetadata, rgb_to_lms_matrix), 100);
    assert_eq!(offset_of!(sys::ColorMetadata, signal_eotf), 172);
    assert_eq!(offset_of!(sys::ColorMetadata, source_max_pq), 190);
    assert_eq!(offset_of!(sys::ColorMetadata, source_diagonal), 192);
    assert_eq!(size_of::<sys::ColorMetadata>(), 196);
}

/// Codec parameters with `bytes` as their side data of `kind`.
fn parameters(kind: ffi::AVPacketSideDataType, bytes: &[u8]) -> codec::Parameters {
    let mut parameters = codec::Parameters::new();
    // SAFETY: FFmpeg allocates the side data as big as asked, owned by the
    // parameters.
    unsafe {
        let raw = parameters.as_mut_ptr();
        let side = ffi::av_packet_side_data_new(
            &raw mut (*raw).coded_side_data,
            &raw mut (*raw).nb_coded_side_data,
            kind,
            bytes.len(),
            0,
        );
        assert!(!side.is_null());
        ptr::copy_nonoverlapping(bytes.as_ptr(), (*side).data, bytes.len());
    }
    parameters
}

/// A light level's bytes, as FFmpeg keeps them.
fn light_bytes(max_cll: u32, max_fall: u32) -> Vec<u8> {
    [max_cll.to_ne_bytes(), max_fall.to_ne_bytes()].concat()
}

/// A mastering display's bytes, as FFmpeg keeps them: its peak `nits`,
/// and nothing else.
fn display_bytes(nits: i32) -> Vec<u8> {
    // SAFETY: all zeros is a display that says nothing.
    let mut display: sys::MasteringDisplay = unsafe { std::mem::zeroed() };
    display.max_luminance = ffi::AVRational { num: nits, den: 1 };
    display.has_luminance = 1;
    bytes_of(&display).to_vec()
}

#[test]
fn a_streams_side_data_is_read_as_big_as_its_struct() {
    let kind = ffi::AVPacketSideDataType::AV_PKT_DATA_CONTENT_LIGHT_LEVEL;
    let whole = parameters(kind, &light_bytes(1000, 400));
    let light = coded::<sys::ContentLightLevel>(&whole).expect("whole");
    assert_eq!((light.max_cll, light.max_fall), (1000, 400));
    assert!(has_coded::<sys::ContentLightLevel>(&whole));

    let display = parameters(
        ffi::AVPacketSideDataType::AV_PKT_DATA_MASTERING_DISPLAY_METADATA,
        &display_bytes(4000),
    );
    let read = coded::<sys::MasteringDisplay>(&display).expect("a display");
    assert_eq!((read.max_luminance.num, read.has_luminance), (4000, 1));
    assert!(coded::<sys::ContentLightLevel>(&display).is_none());

    let short = parameters(kind, &light_bytes(1000, 400)[..7]);
    assert!(
        coded::<sys::ContentLightLevel>(&short).is_none(),
        "cut short"
    );
    assert!(has_coded::<sys::ContentLightLevel>(&short));

    let other = parameters(
        ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF,
        &light_bytes(1000, 400),
    );
    assert!(
        coded::<sys::ContentLightLevel>(&other).is_none(),
        "another kind"
    );
    assert!(!has_coded::<sys::ContentLightLevel>(&other));
    let none = codec::Parameters::new();
    assert!(coded::<sys::ContentLightLevel>(&none).is_none());
    assert!(!has_coded::<sys::ContentLightLevel>(&none));
}

/// A frame with `bytes` as its side data of `kind`.
fn with_side_data(frame: &mut frame::Video, kind: ffi::AVFrameSideDataType, bytes: &[u8]) {
    // SAFETY: FFmpeg allocates the side data as big as asked, owned by the
    // frame.
    unsafe {
        let side = ffi::av_frame_new_side_data(frame.as_mut_ptr(), kind, bytes.len());
        assert!(!side.is_null());
        ptr::copy_nonoverlapping(bytes.as_ptr(), (*side).data, bytes.len());
    }
}

#[test]
fn a_frames_side_data_is_read_as_big_as_its_struct() {
    let kind = ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL;
    let mut whole = frame::Video::new(Pixel::YUV420P, 2, 2);
    assert!(frame::<sys::ContentLightLevel>(&whole).is_none());
    with_side_data(&mut whole, kind, &light_bytes(1000, 400));
    let light = frame::<sys::ContentLightLevel>(&whole).expect("whole");
    assert_eq!((light.max_cll, light.max_fall), (1000, 400));
    assert!(
        frame::<sys::MasteringDisplay>(&whole).is_none(),
        "another kind"
    );
    let display_kind = ffi::AVFrameSideDataType::AV_FRAME_DATA_MASTERING_DISPLAY_METADATA;
    with_side_data(&mut whole, display_kind, &display_bytes(4000));
    let display = frame::<sys::MasteringDisplay>(&whole).expect("a display");
    assert_eq!(
        (display.max_luminance.num, display.has_luminance),
        (4000, 1)
    );
    assert_eq!(
        frame::<sys::ContentLightLevel>(&whole).unwrap().max_cll,
        1000
    );

    let mut short = frame::Video::new(Pixel::YUV420P, 2, 2);
    with_side_data(&mut short, kind, &light_bytes(1000, 400)[..7]);
    assert!(
        frame::<sys::ContentLightLevel>(&short).is_none(),
        "cut short"
    );
}

/// The bytes of `value`, as it is in memory.
fn bytes_of<T>(value: &T) -> &[u8] {
    // SAFETY: a value's own bytes, all of them, while it's borrowed.
    unsafe { std::slice::from_raw_parts(ptr::from_ref(value).cast(), size_of::<T>()) }
}

/// An RPU's side data with its parts at these offsets, its header saying
/// `profile`, and `len` bytes long.
fn rpu_bytes(offsets: [usize; 3], profile: u8, len: usize) -> Vec<u8> {
    let mut bytes = vec![0; len];
    let metadata = sys::DoviMetadata {
        header_offset: offsets[0],
        mapping_offset: offsets[1],
        color_offset: offsets[2],
    };
    bytes[..size_of::<sys::DoviMetadata>()].copy_from_slice(bytes_of(&metadata));
    let at = offsets[0].wrapping_add(offset_of!(sys::RpuDataHeader, vdr_rpu_profile));
    if let Some(byte) = bytes.get_mut(at) {
        *byte = profile;
    }
    bytes
}

/// Offsets for an RPU's parts, each at a multiple of 8 after the last, as
/// FFmpeg places them, and how long they come to.
fn placed() -> ([usize; 3], usize) {
    let after = |offset: usize, len: usize| (offset + len).next_multiple_of(8);
    let header = after(0, size_of::<sys::DoviMetadata>());
    let mapping = after(header, size_of::<sys::RpuDataHeader>());
    let color = after(mapping, size_of::<sys::DataMapping>());
    (
        [header, mapping, color],
        color + size_of::<sys::ColorMetadata>(),
    )
}

/// A frame with an RPU of `bytes`.
fn rpu_frame(bytes: &[u8]) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::YUV420P10LE, 2, 2);
    let kind = ffi::AVFrameSideDataType::AV_FRAME_DATA_DOVI_METADATA;
    with_side_data(&mut frame, kind, bytes);
    frame
}

/// Where `frame`'s RPU starts, to say where its parts are read from.
fn rpu_start(frame: &frame::Video) -> usize {
    // SAFETY: the frame is valid while borrowed; its side data is looked
    // up, not read.
    unsafe {
        let side = ffi::av_frame_get_side_data(
            frame.as_ptr(),
            ffi::AVFrameSideDataType::AV_FRAME_DATA_DOVI_METADATA,
        );
        (*side).data as usize
    }
}

#[test]
fn an_rpu_is_read_where_its_side_data_says() {
    assert!(rpu(&frame::Video::new(Pixel::YUV420P10LE, 2, 2)).is_none());
    let (offsets, len) = placed();
    let frame = rpu_frame(&rpu_bytes(offsets, 7, len));
    let read = rpu(&frame).expect("an RPU");
    assert_eq!(read.header.vdr_rpu_profile, 7);
    let at = |part: *const u8| part as usize - rpu_start(&frame);
    assert_eq!(at(ptr::from_ref(read.header).cast()), offsets[0]);
    assert_eq!(at(ptr::from_ref(read.mapping).cast()), offsets[1]);
    assert_eq!(at(ptr::from_ref(read.color).cast()), offsets[2]);
}

#[test]
fn an_rpu_with_a_part_beyond_its_side_data_is_none() {
    let (offsets, len) = placed();
    let frame = rpu_frame(&rpu_bytes(offsets, 7, len - 1));
    assert!(rpu(&frame).is_none(), "its colors cut short");
    for part in 0..3 {
        let mut beyond = offsets;
        // Placed as any part may be, so only its size can rule it out.
        beyond[part] = usize::MAX - 7;
        let frame = rpu_frame(&rpu_bytes(beyond, 7, len));
        assert!(rpu(&frame).is_none(), "part {part} beyond it");
    }
    let frame = rpu_frame(&[0; 23]);
    assert!(rpu(&frame).is_none(), "where its parts are, cut short");
}

#[test]
fn an_rpu_with_a_part_out_of_place_is_none() {
    let (offsets, len) = placed();
    for part in 0..3 {
        let mut moved = offsets;
        moved[part] += 1;
        let frame = rpu_frame(&rpu_bytes(moved, 7, len + 8));
        assert!(rpu(&frame).is_none(), "part {part} out of place");
    }
}

/// FFmpeg's own matrices, as its demuxers make them, come back as the
/// turns and mirroring they ask for.
#[test]
fn reads_display_matrices_as_turns_and_mirroring() {
    let read = |rotation: f64, flipped: bool| {
        let mut matrix = [0i32; 9];
        // SAFETY: both write nine i32s.
        unsafe {
            ffi::av_display_rotation_set(matrix.as_mut_ptr(), rotation);
            ffi::av_display_matrix_flip(matrix.as_mut_ptr(), flipped.into(), 0);
        }
        DisplayMatrix(matrix).turns()
    };
    // `av_display_rotation_set` takes degrees clockwise, while
    // `av_display_rotation_get` (and ffprobe) give them counterclockwise:
    // a phone held upright records -90, and players turn it a quarter
    // clockwise.
    assert_eq!(read(0.0, false), Some((0, false)));
    assert_eq!(read(90.0, false), Some((1, false)));
    assert_eq!(read(-90.0, false), Some((3, false)));
    assert_eq!(read(180.0, false), Some((2, false)));
    assert_eq!(read(88.0, false), Some((1, false)));
    let mut phone = [0i32; 9];
    // SAFETY: both take nine i32s.
    let rotation = unsafe {
        ffi::av_display_rotation_set(phone.as_mut_ptr(), 90.0);
        ffi::av_display_rotation_get(phone.as_ptr())
    };
    assert_eq!(rotation.round(), -90.0);
    // The flip comes after the turn: mirrored first, it turns the other
    // way.
    assert_eq!(read(0.0, true), Some((0, true)));
    assert_eq!(read(90.0, true), Some((3, true)));
    assert_eq!(read(180.0, true), Some((2, true)));
    assert_eq!(read(-90.0, true), Some((1, true)));
    assert_eq!(DisplayMatrix([0; 9]).turns(), None);
}

#[test]
fn a_streams_display_matrix_is_read_from_its_side_data() {
    let mut matrix = [0i32; 9];
    // SAFETY: it writes nine i32s.
    unsafe { ffi::av_display_rotation_set(matrix.as_mut_ptr(), 90.0) };
    let bytes: Vec<u8> = matrix.iter().flat_map(|n| n.to_ne_bytes()).collect();
    let kind = ffi::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX;
    assert_eq!(
        coded::<DisplayMatrix>(&parameters(kind, &bytes)),
        Some(DisplayMatrix(matrix))
    );
    assert_eq!(
        coded::<DisplayMatrix>(&parameters(kind, &bytes[..35])),
        None
    );
}

#[test]
fn a_streams_dolby_vision_configuration_says_which_layers_it_has() {
    let kind = ffi::AVPacketSideDataType::AV_PKT_DATA_DOVI_CONF;
    // Version 1.0, profile 7, level 6, an RPU or not, then the enhancement
    // layer and the base layer.
    let record = |rpu, el, bl| parameters(kind, &[1, 0, 7, 6, rpu, el, bl, 6, 0]);
    assert_eq!(
        DolbyVision::of(&record(1, 1, 1)),
        Some(DolbyVision {
            profile: 7,
            level: 6,
            compatibility: 6,
            rpu: true,
            enhancement: true,
            base: true,
        })
    );
    let read = |rpu, el, bl| DolbyVision::of(&record(rpu, el, bl)).unwrap();
    assert_eq!(
        (
            read(0, 1, 1).rpu,
            read(1, 0, 1).enhancement,
            read(1, 1, 0).base
        ),
        (false, false, false)
    );
    assert!(!read(1, 1, 1).is_layer_only());
    assert!(read(1, 1, 0).is_layer_only());
    assert!(!read(1, 0, 1).is_layer_only());
    assert!(DolbyVision::is_tagged(&record(1, 1, 1)));

    let short = parameters(kind, &[1, 0, 7, 6, 1, 1, 0, 6]);
    assert_eq!(DolbyVision::of(&short), None, "cut short");
    assert!(DolbyVision::is_tagged(&short));
    let none = codec::Parameters::new();
    assert_eq!(DolbyVision::of(&none), None);
    assert!(!DolbyVision::is_tagged(&none));
}
