//! HDR video, PQ (HDR10) and HLG pictures in BT.2020, fitted to an SDR
//! screen on the CPU, for software decoding and Linux: the light's maths
//! are the `hdr` crate's, which the stream server shares. gpui's Metal
//! shader for HDR surfaces (`surface_hdr_fragment` in vendor/gpui) does the
//! same on the GPU, under the screen's EDR headroom. Dolby Vision's
//! profiles 5 and 20 need more before they are HDR like the rest (`dovi`).

use anyhow::{Result, ensure};
use ffmpeg::{ffi, frame};
use ffmpeg_next as ffmpeg;
use ffmpeg_safe::sys;

pub use ::hdr::{
    REFERENCE_WHITE_NITS, Tables, Transfer, encode, hlg_nits_to_scene, hlg_scene_to_nits,
    hlg_to_scene, nits_to_pq, peak_nits, pq_to_nits, scene_to_hlg, sdr_to_hdr, srgb_decode,
    srgb_encode,
};
use ::hdr::{bt2020_to_bt709, tone_map};

use super::picture::{apply_gamma, bgra_rows};

/// Give a frame read back from the GPU the peak its video showed with, as
/// its MaxCLL, for `peak_nits` to find again.
pub fn attach_peak(frame: &mut frame::Video, nits: f32) -> Result<()> {
    // SAFETY: the side data FFmpeg makes for the frame is as big as asked,
    // and this kind is `ContentLightLevel`.
    unsafe {
        let side = ffi::av_frame_new_side_data(
            frame.as_mut_ptr(),
            ffi::AVFrameSideDataType::AV_FRAME_DATA_CONTENT_LIGHT_LEVEL,
            size_of::<sys::ContentLightLevel>(),
        );
        ensure!(
            !side.is_null() && !(*side).data.is_null(),
            "no memory for a frame's peak"
        );
        (*side)
            .data
            .cast::<sys::ContentLightLevel>()
            .write(sys::ContentLightLevel {
                max_cll: nits.round() as u32,
                max_fall: 0,
            });
    }
    Ok(())
}

/// Steps in the encoding table, enough that dark tones don't band.
const ENCODE_STEPS: usize = 16384;

/// Turns HDR pictures, as nonlinear BT.2020 RGB48 from swscale, into 8-bit
/// BGRA for an SDR screen.
pub struct ToneMapper {
    transfer: Transfer,
    tables: &'static Tables,
    /// Linear light in 0..=1 to an sRGB byte, with `gamma` applied.
    encode: Vec<u8>,
    gamma: f32,
    /// The brightest pictures get, relative to SDR white, if it's known.
    peak: Option<f32>,
}

impl ToneMapper {
    pub fn new(transfer: Transfer) -> Self {
        Self {
            transfer,
            tables: Tables::get(),
            encode: Self::encode_table(1.0),
            gamma: 1.0,
            peak: None,
        }
    }

    fn encode_table(gamma: f32) -> Vec<u8> {
        (0..=ENCODE_STEPS)
            .map(|i| {
                let signal = srgb_encode(i as f32 / ENCODE_STEPS as f32);
                (apply_gamma(signal, gamma) * 255.0).round() as u8
            })
            .collect()
    }

    /// Fit pictures from now on to their video's peak, in nits
    /// (`peak_nits`), if it's known.
    pub fn set_peak(&mut self, nits: Option<f32>) {
        self.peak = nits.map(|nits| nits / REFERENCE_WHITE_NITS);
    }

    /// Apply `gamma` (see `picture::apply_gamma`) to pictures from now on.
    /// For an SDR screen it comes after tone mapping, as on the GPU.
    pub fn set_gamma(&mut self, gamma: f32) {
        if gamma != self.gamma {
            self.encode = Self::encode_table(gamma);
            self.gamma = gamma;
        }
    }

    /// One pixel, from 16-bit nonlinear BT.2020 RGB to BGRA.
    fn pixel(&self, rgb: [u16; 3]) -> [u8; 4] {
        let signal = rgb.map(|c| f32::from(c) / 65535.0);
        let nits = match self.transfer {
            Transfer::Pq => signal.map(|signal| self.tables.pq_nits(signal)),
            Transfer::Hlg => hlg_scene_to_nits(signal.map(|signal| self.tables.hlg_scene(signal))),
        };
        self.output(nits.map(|nits| nits / REFERENCE_WHITE_NITS))
    }

    /// Linear light relative to SDR white, for a PQ signal in 0..=1.
    pub fn pq_light(&self, signal: f32) -> f32 {
        self.tables.pq_nits(signal) / REFERENCE_WHITE_NITS
    }

    /// Linear BT.2020 light, relative to SDR white, to a BGRA pixel.
    pub fn output(&self, light: [f32; 3]) -> [u8; 4] {
        let [r, g, b] = tone_map(bt2020_to_bt709(light), 1.0, self.peak);
        let encode = |c: f32| self.encode[(c.clamp(0.0, 1.0) * ENCODE_STEPS as f32 + 0.5) as usize];
        [encode(b), encode(g), encode(r), 255]
    }

    /// Convert a `width` x `height` RGB48LE picture, `stride` bytes a row,
    /// into tightly packed BGRA.
    pub fn run(&self, data: &[u8], stride: usize, width: usize, height: usize) -> Vec<u8> {
        bgra_rows(width, height, |y, out| {
            let row = &data[y * stride..][..width * 6];
            let (pixels, _) = row.as_chunks::<6>();
            let (outs, _) = out.as_chunks_mut::<4>();
            for (pixel, out) in pixels.iter().zip(outs) {
                let sample = |i: usize| u16::from_le_bytes([pixel[2 * i], pixel[2 * i + 1]]);
                *out = self.pixel([sample(0), sample(1), sample(2)]);
            }
        })
    }
}

#[cfg(test)]
#[path = "tests/hdr.rs"]
mod tests;
