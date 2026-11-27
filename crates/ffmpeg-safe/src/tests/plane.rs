use ffmpeg_next::format::Pixel;
use ffmpeg_next::{ffi, frame};

use super::{bottom_up, plane, plane_mut};

/// The lengths and strides of `frame`'s first four planes.
fn extents(frame: &frame::Video) -> [Option<(usize, usize)>; 4] {
    std::array::from_fn(|index| plane(frame, index).map(|(data, stride)| (data.len(), stride)))
}

#[test]
fn slices_each_plane_to_the_end_of_its_last_rows_picture() {
    let frame = frame::Video::new(Pixel::YUV420P, 7, 5);
    let [luma, blue, red, none] = extents(&frame);
    let (length, stride) = luma.unwrap();
    assert!(stride >= 7);
    assert_eq!(length, stride * 4 + 7);
    // Chroma is half as wide and high, rounded up.
    let (length, stride) = blue.unwrap();
    assert_eq!(length, stride * 2 + 4);
    assert_eq!(red, blue);
    assert_eq!(none, None);
    // Ten bits take two bytes a sample; NV12's chroma, two samples a pixel.
    let deep = frame::Video::new(Pixel::YUV420P10LE, 8, 4);
    let (length, stride) = extents(&deep)[0].unwrap();
    assert_eq!(length, stride * 3 + 16);
    let nv12 = frame::Video::new(Pixel::NV12, 8, 4);
    let [_, chroma, none, _] = extents(&nv12);
    let (length, stride) = chroma.unwrap();
    assert_eq!(length, stride + 8);
    assert_eq!(none, None);
    // Past FFmpeg's four, nothing.
    assert!(plane(&frame, 4).is_none());
}

#[test]
fn slices_a_cropped_frame_within_its_pictures() {
    let mut frame = frame::Video::new(Pixel::YUV420P, 16, 8);
    let (whole, stride) = plane(&frame, 0).unwrap();
    let end = whole.as_ptr_range().end;
    // SAFETY: the frame is ours; cropping moves its planes' starts in and
    // shrinks it, within what it has.
    unsafe {
        let raw = frame.as_mut_ptr();
        (*raw).crop_left = 4;
        (*raw).crop_top = 2;
        assert_eq!(
            ffi::av_frame_apply_cropping(raw, ffi::AV_FRAME_CROP_UNALIGNED as i32),
            0
        );
    }
    let (cropped, cropped_stride) = plane(&frame, 0).unwrap();
    assert_eq!(cropped_stride, stride);
    assert_eq!(cropped.len(), stride * 5 + 12);
    assert_eq!(
        cropped.as_ptr_range().end,
        end,
        "no further than the whole one"
    );
}

#[test]
fn slices_no_plane_stored_bottom_up() {
    let mut frame = frame::Video::new(Pixel::YUV420P10LE, 8, 4);
    assert!(!bottom_up(&frame));
    // SAFETY: the frame is ours; each plane is pointed at its last row and
    // read backwards, as a decoder flipping it does. FFmpeg frees its
    // buffers, not where its planes start.
    unsafe {
        let raw = frame.as_mut_ptr();
        for index in 0..3 {
            let rows = if index == 0 { 4 } else { 2 };
            let linesize = (*raw).linesize[index];
            (*raw).data[index] = (*raw).data[index].offset((linesize * (rows - 1)) as isize);
            (*raw).linesize[index] = -linesize;
        }
    }
    assert!(bottom_up(&frame));
    assert_eq!(extents(&frame), [None; 4]);
    assert!(plane_mut(&mut frame, 0).is_none());
    // Any one of its planes makes it so; the others are sliced.
    let mut frame = frame::Video::new(Pixel::YUV420P10LE, 8, 4);
    // SAFETY: the frame is ours; only its linesize is turned, and nothing
    // reads the plane by it.
    unsafe { (*frame.as_mut_ptr()).linesize[2] *= -1 };
    assert!(bottom_up(&frame));
    let [luma, blue, red, _] = extents(&frame);
    assert!(luma.is_some() && blue.is_some());
    assert_eq!(red, None);
}

#[test]
fn slices_no_palette_hardware_surface_or_unknown_format() {
    let palette = frame::Video::new(Pixel::PAL8, 8, 4);
    let [pixels, colors, _, _] = extents(&palette);
    assert!(pixels.is_some());
    assert_eq!(colors, None);
    let mut frame = frame::Video::new(Pixel::YUV420P, 8, 4);
    for format in [
        ffi::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX as i32,
        ffi::AVPixelFormat::AV_PIX_FMT_NB as i32,
        9999,
        -1,
    ] {
        // SAFETY: the frame is ours, its format a plain number.
        unsafe { (*frame.as_mut_ptr()).format = format };
        assert_eq!(extents(&frame), [None; 4], "{format}");
    }
    // No pictures at all.
    assert_eq!(extents(&frame::Video::empty()), [None; 4]);
    // Rows further apart than a linesize says.
    let mut frame = frame::Video::new(Pixel::GRAY8, 8, 4);
    // SAFETY: the frame is ours, its linesize a plain number.
    unsafe { (*frame.as_mut_ptr()).linesize[0] = 4 };
    assert_eq!(extents(&frame)[0], None);
}

#[test]
fn writes_into_a_copy_of_pictures_another_frame_shares() {
    let mut first = frame::Video::new(Pixel::GRAY8, 4, 2);
    plane_mut(&mut first, 0).unwrap().0.fill(10);
    let mut second = frame::Video::empty();
    // SAFETY: both frames are valid, the second empty; it refers to the
    // first's pictures, which FFmpeg counts references to.
    let referred = unsafe { ffi::av_frame_ref(second.as_mut_ptr(), first.as_ptr()) };
    assert_eq!(referred, 0);
    let (written, _) = plane_mut(&mut second, 0).unwrap();
    written.fill(200);
    assert!(plane(&first, 0).unwrap().0.iter().all(|&byte| byte == 10));
    let (data, stride) = plane(&second, 0).unwrap();
    assert!(data.iter().all(|&byte| byte == 200));
    assert_eq!(data.len(), stride + 4);
}
