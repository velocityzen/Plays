//! Brightness fitted under what a screen can show, its headroom over SDR
//! white: everything below three quarters of it left alone, the rest
//! rolled off smoothly toward it, so nothing clips. How far goes by the
//! video's peak, where its metadata says what that is (`peak_nits`): up to
//! it, the highlights spread over the room left, and a video that fits
//! under the headroom is left alone.

use ffmpeg_next::frame;
use ffmpeg_safe::sys;

use crate::{HLG_PEAK_NITS, KNEE, PQ_PEAK_NITS, REFERENCE_WHITE_NITS, Transfer};

/// Brightness relative to SDR white, fitted under `headroom`: unchanged up
/// to the knee, then rolled off. With the video's `peak` (relative to SDR
/// white too) it reaches the headroom there, the highlights spread over the
/// room above the knee (Reinhard's curve with a white point), and a video
/// whose peak fits under the headroom is left alone; without, it eases
/// toward the headroom without reaching it. The slope is 1 on both sides of
/// the knee, so there is no visible seam.
pub fn tone_curve(x: f32, headroom: f32, peak: Option<f32>) -> f32 {
    let knee = KNEE * headroom;
    if x <= knee {
        return x;
    }
    let room = headroom - knee;
    let past = (x - knee) / room;
    let eased = match peak {
        Some(peak) => {
            // How many rooms past the knee the peak is: one if it fits.
            let white = ((peak - knee) / room).max(1.0);
            (past * (1.0 + past / (white * white)) / (1.0 + past)).min(1.0)
        }
        None => 1.0 - (-past).exp(),
    };
    knee + room * eased
}

/// Fit a color's brightness under `headroom` without shifting its hue: the
/// brightest channel follows the tone curve and the others scale with it.
pub fn tone_map(rgb: [f32; 3], headroom: f32, peak: Option<f32>) -> [f32; 3] {
    let brightest = rgb[0].max(rgb[1]).max(rgb[2]);
    if brightest <= 0.0 {
        return rgb;
    }
    let scale = tone_curve(brightest, headroom, peak) / brightest;
    rgb.map(|c| c * scale)
}

/// The brightest `frame`'s video gets, in nits, as far as anything says.
/// HLG is always as on its nominal display. PQ's HDR10 metadata says: the
/// brightest pixel of the whole video (MaxCLL), or the peak of the display
/// it was mastered on, whichever is lower when it gives both. Dolby
/// Vision's IPT says in its RPU (`Reshaping::peak_nits`).
pub fn peak_nits(frame: &frame::Video) -> Option<f32> {
    match Transfer::of(frame)? {
        Transfer::Hlg => Some(HLG_PEAK_NITS),
        Transfer::Pq => {
            let light = ffmpeg_safe::frame::<sys::ContentLightLevel>(frame);
            let display = ffmpeg_safe::frame::<sys::MasteringDisplay>(frame);
            let brightest = light.map(|light| light.max_cll as f32);
            // Over nothing, a peak is no number, which isn't plausible.
            let mastered = display
                .filter(|display| display.has_luminance != 0)
                .map(|display| display.max_luminance.num as f32 / display.max_luminance.den as f32);
            [brightest, mastered]
                .into_iter()
                .flatten()
                .filter_map(plausible_peak)
                .reduce(f32::min)
        }
    }
}

/// `nits` as a video's peak, unless it's under reference white or over
/// PQ's, which no video is mastered for: then it says nothing.
pub fn plausible_peak(nits: f32) -> Option<f32> {
    (REFERENCE_WHITE_NITS..=PQ_PEAK_NITS)
        .contains(&nits)
        .then_some(nits)
}

#[cfg(test)]
#[path = "tests/tone.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/tone_properties.rs"]
mod properties;
