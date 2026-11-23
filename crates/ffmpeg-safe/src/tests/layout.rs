use std::ffi::CString;

use ffmpeg_next::format::Sample;
use ffmpeg_next::format::sample::Type;
use ffmpeg_next::{ChannelLayout, ffi, frame};

use super::Layout;

/// A frame of two channels whose layout FFmpeg reads from `name`.
fn frame_of(name: &str) -> frame::Audio {
    let mut frame = frame::Audio::new(Sample::I16(Type::Packed), 16, ChannelLayout::STEREO);
    let name = CString::new(name).unwrap();
    // SAFETY: the frame is valid, and its layout its own to replace.
    unsafe {
        let layout = &mut (*frame.as_mut_ptr()).ch_layout;
        ffi::av_channel_layout_uninit(layout);
        assert_eq!(ffi::av_channel_layout_from_string(layout, name.as_ptr()), 0);
    }
    frame
}

#[test]
fn a_layout_in_an_uncommon_order_outlives_its_frame() {
    let mut frame = frame_of("FR+FL");
    let layout = Layout::of(&mut frame);
    drop(frame);
    assert_eq!(format!("{layout:?}"), "2 channels (FR+FL)");
    assert_eq!(layout, Layout::of(&mut frame_of("FR+FL")));
    assert_ne!(layout, Layout::of(&mut frame_of("FL+FR")));
}

#[test]
fn a_frame_with_no_layout_is_given_the_usual_one() {
    let mut frame = frame_of("2C");
    let layout = Layout::of(&mut frame);
    assert_eq!(format!("{layout:?}"), "stereo");
    // SAFETY: the frame is valid.
    let given = unsafe { (*frame.as_ptr()).ch_layout };
    assert_eq!(given.order, ffi::AVChannelOrder::AV_CHANNEL_ORDER_NATIVE);
    // SAFETY: a layout in native order is a mask.
    assert_eq!(unsafe { given.u.mask }, ffi::AV_CH_LAYOUT_STEREO);
}
