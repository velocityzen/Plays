//! A video frame's planes, sliced soundly. ffmpeg-next takes a plane's
//! linesize for a `usize` and slices a whole linesize a row: a frame stored
//! bottom up, whose linesize is negative, makes a slice far longer than
//! memory, and a cropped one's last row can run past its buffer.

use std::slice;

use ffmpeg_next::{ffi, frame};

/// `frame`'s plane `index`: its bytes, and the stride from the start of
/// one row to the next. Its last row is only as long as the picture is
/// wide. None where the frame has no such plane, keeps it on a hardware
/// surface or as a palette, or stores it bottom up (`bottom_up`).
pub fn plane(frame: &frame::Video, index: usize) -> Option<(&[u8], usize)> {
    let (length, stride) = extent(frame, index)?;
    // SAFETY: `extent` found the plane's start, and by FFmpeg's own sums
    // its rows, within the frame's pictures, borrowed with it.
    let data = unsafe { slice::from_raw_parts((*frame.as_ptr()).data[index], length) };
    Some((data, stride))
}

/// `frame`'s plane `index`, as `plane` gives it, to write. A frame sharing
/// its pictures with another gets a copy of its own first, so the other
/// is left as it is.
pub fn plane_mut(frame: &mut frame::Video, index: usize) -> Option<(&mut [u8], usize)> {
    extent(frame, index)?;
    // SAFETY: the frame is valid and borrowed mutably; FFmpeg copies its
    // pictures if another frame refers to them too.
    if unsafe { ffi::av_frame_make_writable(frame.as_mut_ptr()) } < 0 {
        return None;
    }
    let (length, stride) = extent(frame, index)?;
    // SAFETY: as in `plane`; the pictures are this frame's alone, and
    // borrowed mutably with it.
    let data = unsafe { slice::from_raw_parts_mut((*frame.as_mut_ptr()).data[index], length) };
    Some((data, stride))
}

/// Whether `frame` stores a plane bottom up, its rows going back through
/// memory (a negative linesize): `plane` slices none of those, and
/// swscale turns them the right way up.
pub fn bottom_up(frame: &frame::Video) -> bool {
    // SAFETY: the frame is valid while borrowed, its linesizes numbers.
    let linesizes = unsafe { (*frame.as_ptr()).linesize };
    linesizes[..4].iter().any(|&linesize| linesize < 0)
}

/// Plane `index`'s length in bytes, to the end of its last row's picture,
/// and its stride.
fn extent(frame: &frame::Video, index: usize) -> Option<(usize, usize)> {
    if index >= 4 {
        return None;
    }
    // SAFETY: the frame is valid while borrowed. Its format is read as the
    // number it is, and made a pixel format only where FFmpeg has one by
    // that number; FFmpeg writes four numbers into each array.
    unsafe {
        let raw = frame.as_ptr();
        if (*raw).data[index].is_null() {
            return None;
        }
        let stride = usize::try_from((*raw).linesize[index]).ok()?;
        let format = (*raw).format;
        if !(0..ffi::AVPixelFormat::AV_PIX_FMT_NB as i32).contains(&format) {
            return None;
        }
        let format = std::mem::transmute::<i32, ffi::AVPixelFormat>(format);
        // The bytes a row of the picture takes in each plane, then each
        // plane's rows of them: FFmpeg's sums, which refuse hardware
        // surfaces.
        let mut row = [0; 4];
        if ffi::av_image_fill_linesizes(row.as_mut_ptr(), format, (*raw).width) < 0 {
            return None;
        }
        let rows = row.map(|bytes| bytes as isize);
        let mut sizes = [0; 4];
        if ffi::av_image_fill_plane_sizes(sizes.as_mut_ptr(), format, (*raw).height, rows.as_ptr())
            < 0
        {
            return None;
        }
        let row = usize::try_from(row[index]).ok().filter(|&row| row > 0)?;
        let size = sizes[index];
        if size == 0 || size % row != 0 || row > stride {
            return None;
        }
        Some((stride * (size / row - 1) + row, stride))
    }
}

#[cfg(test)]
#[path = "tests/plane.rs"]
mod tests;
