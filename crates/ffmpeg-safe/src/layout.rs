//! A frame's channel layout, copied out of it. ffmpeg-next's
//! `ChannelLayout` is a bare copy of FFmpeg's: one in an uncommon order
//! (custom, as FFmpeg says) points into the frame, which frees that when
//! it is used for the next.

use std::ffi::{CStr, c_char};
use std::fmt;
use std::mem::MaybeUninit;

use ffmpeg_next::{ChannelLayout, ffi, frame};

/// A channel layout of its own, freed with it, and compared by FFmpeg.
pub struct Layout(ffi::AVChannelLayout);

// SAFETY: the layout, and the map an uncommon one has, are this one's alone.
unsafe impl Send for Layout {}
// SAFETY: shared, it is only read.
unsafe impl Sync for Layout {}

impl Layout {
    /// The layout of `frame`. A decoder may leave it unspecified, which
    /// resampling can't take: the frame is then given the usual layout
    /// for as many channels.
    pub fn of(frame: &mut frame::Audio) -> Self {
        // SAFETY: the frame is valid, and its layout its own to replace.
        unsafe {
            let layout = &mut (*frame.as_mut_ptr()).ch_layout;
            if layout.order == ffi::AVChannelOrder::AV_CHANNEL_ORDER_UNSPEC {
                let channels = layout.nb_channels;
                ffi::av_channel_layout_uninit(layout);
                ffi::av_channel_layout_default(layout, channels);
            }
            Self::copy(layout)
        }
    }

    /// # Safety
    /// `layout` is valid.
    unsafe fn copy(layout: &ffi::AVChannelLayout) -> Self {
        // Zeroed is an empty layout, which copying into first uninits.
        let mut own = MaybeUninit::<ffi::AVChannelLayout>::zeroed();
        // SAFETY: both are valid layouts.
        let copied = unsafe { ffi::av_channel_layout_copy(own.as_mut_ptr(), layout) };
        assert!(copied >= 0, "no memory to copy a channel layout");
        // SAFETY: copied in.
        Self(unsafe { own.assume_init() })
    }

    /// This layout for an FFmpeg call that copies it, as making a
    /// resampler does. ffmpeg-next keeps what it's given as it is, and
    /// that points into this one: compare `Layout`s, never what it kept.
    pub fn lent(&self) -> ChannelLayout {
        ChannelLayout::from(self.0)
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        // SAFETY: the layout is this one's own, freed once.
        unsafe { ffi::av_channel_layout_uninit(&mut self.0) };
    }
}

impl PartialEq for Layout {
    fn eq(&self, other: &Self) -> bool {
        // SAFETY: both are valid. 0 is the same; 1 not, and less an error.
        unsafe { ffi::av_channel_layout_compare(&self.0, &other.0) == 0 }
    }
}

impl Eq for Layout {}

/// FFmpeg's name for it, as `stereo` or `2 channels (FR+FL)`.
impl fmt::Debug for Layout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut name = [0 as c_char; 128];
        // SAFETY: the layout is valid, and the buffer as long as said.
        let written =
            unsafe { ffi::av_channel_layout_describe(&self.0, name.as_mut_ptr(), name.len()) };
        if written < 0 {
            return f.write_str("?");
        }
        // SAFETY: FFmpeg ends it with a nul, within the buffer.
        let name = unsafe { CStr::from_ptr(name.as_ptr()) };
        f.write_str(&name.to_string_lossy())
    }
}
