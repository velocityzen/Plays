//! Dolby Vision without an HDR10 base layer (profile 5, as streaming
//! services use, and profile 20's stereo MV-HEVC, of which FFmpeg decodes
//! the first view). Its pictures are IPT, the base layer of Dolby's
//! IPTPQc2, and only look right after the reshaping that comes with every
//! frame in its RPU: the `hdr` crate's `Reshaping`, into linear BT.2020
//! light, from where `hdr` takes over.
//!
//! gpui's Metal shader for Dolby Vision surfaces (`surface_dovi_fragment`
//! in vendor/gpui) reshapes on the GPU, by `Reshaping::shader_params`.
//! This is the CPU version, for software decoding and Linux.

use ffmpeg::frame;
use ffmpeg::util::color;
use ffmpeg_next as ffmpeg;
use ffmpeg_safe::Colors;

pub use ::hdr::Reshaping;

use super::hdr::{ToneMapper, Transfer};
use super::picture::bgra_rows;

/// The frame's reshaping if it needs one: it is Dolby Vision and its
/// pictures are IPT, untagged (profile 5) or tagged IPT-C2 though PQ too
/// (profile 20). Profile 8 and the like carry an HDR10 or HLG picture,
/// tagged as such, which is shown as it is.
pub fn reshaping(frame: &frame::Video) -> Option<Reshaping> {
    if Colors::of_frame(frame).space != color::Space::IPT_C2 && Transfer::of(frame).is_some() {
        return None;
    }
    Reshaping::of(frame)
}

/// Convert a `width` x `height` picture, scaled to 16-bit 4:4:4 planes by
/// swscale (`planes` with their row strides in bytes), into tightly packed
/// BGRA for an SDR screen, by `reshaping`. `mapper` is a PQ tone mapper.
pub fn render(
    reshaping: &Reshaping,
    mapper: &ToneMapper,
    planes: [(&[u8], usize); 3],
    width: usize,
    height: usize,
) -> Vec<u8> {
    bgra_rows(width, height, |y, out| {
        let rows = planes.map(|(data, stride)| &data[y * stride..][..width * 2]);
        let (out, _) = out.as_chunks_mut::<4>();
        for (x, out) in out.iter_mut().enumerate() {
            // 16-bit samples hold the base layer's codes in their top bits.
            let samples = rows
                .map(|row| f32::from(u16::from_le_bytes([row[2 * x], row[2 * x + 1]])) / 65536.0);
            let lms = reshaping
                .lms_signal(samples)
                .map(|signal| mapper.pq_light(signal));
            *out = mapper.output(reshaping.lms_to_rgb(lms));
        }
    })
}

// Its frames are other tests' too.
#[cfg(test)]
#[path = "tests/dovi.rs"]
pub(crate) mod tests;
