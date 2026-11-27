//! HDR video's light, for Plays and its stream server alike:
//!
//! - PQ (HDR10) and HLG both ways, BT.2020's and BT.709's primaries, sRGB
//!   (`light`), and by table (`tables`)
//! - brightness fitted under a screen's headroom, by the video's peak
//!   (`tone`)
//! - Dolby Vision's IPT reshaped by its RPU (`dovi`)
//!
//! Brightness is measured against SDR white: HDR's reference white of 203
//! nits (ITU-R BT.2408) lands there. vendor/gpui's shaders do the same on
//! the GPU with the constants here, which tests read back out of them.

mod dovi;
mod light;
mod tables;
mod tone;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;

use ffmpeg_next::frame;
use ffmpeg_next::util::color::TransferCharacteristic;
use ffmpeg_safe::Colors;

pub use dovi::{Reshaping, ShaderParams, mastered_peak};
pub use light::{
    bt709_to_bt2020, bt2020_to_bt709, encode, hlg_nits_to_scene, hlg_scene_to_nits, hlg_to_scene,
    nits_to_pq, pq_to_nits, scene_to_hlg, sdr_to_hdr, srgb_decode, srgb_encode,
};
pub use tables::Tables;
pub use tone::{peak_nits, plausible_peak, tone_curve, tone_map};

/// HDR reference white (BT.2408), shown as SDR white.
pub const REFERENCE_WHITE_NITS: f32 = 203.0;
/// The display HLG is rendered for (BT.2100's nominal 1000 nits), which
/// sets its system gamma of 1.2.
pub const HLG_PEAK_NITS: f32 = 1000.0;
pub const HLG_GAMMA: f32 = 1.2;
/// Brightness below this share of the headroom is left alone.
pub const KNEE: f32 = 0.75;
/// PQ's peak, which no video is mastered over.
pub const PQ_PEAK_NITS: f32 = 10000.0;

/// PQ's constants (SMPTE ST 2084).
pub const PQ_M1: f32 = 2610.0 / 16384.0;
pub const PQ_M2: f32 = 2523.0 / 4096.0 * 128.0;
pub const PQ_C1: f32 = 3424.0 / 4096.0;
pub const PQ_C2: f32 = 2413.0 / 4096.0 * 32.0;
pub const PQ_C3: f32 = 2392.0 / 4096.0 * 32.0;
/// HLG's OETF constants (BT.2100).
pub const HLG_A: f32 = 0.178_832_77;
pub const HLG_B: f32 = 1.0 - 4.0 * HLG_A;
pub const HLG_C: f32 = 0.559_910_7;
/// BT.2020's luma weights of R, G and B.
pub const BT2020_LUMA: [f32; 3] = [0.2627, 0.6780, 0.0593];

/// How an HDR picture codes light.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    /// SMPTE ST 2084: HDR10, HDR10+ and most Dolby Vision.
    Pq,
    /// Hybrid log-gamma (ARIB STD-B67), used by broadcasters.
    Hlg,
}

impl Transfer {
    /// `frame`'s, as its tags say, or `None` for SDR.
    pub fn of(frame: &frame::Video) -> Option<Self> {
        match Colors::of_frame(frame).transfer {
            TransferCharacteristic::SMPTE2084 => Some(Self::Pq),
            TransferCharacteristic::ARIB_STD_B67 => Some(Self::Hlg),
            _ => None,
        }
    }
}
