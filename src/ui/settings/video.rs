//! Video: how the picture fills its tile, its gamma, and decoding.

use gpui::{AnyElement, Context, Window};

use super::form::{group, labeled_slider, row, set, slide};
use super::{Pane, SettingsView};
use crate::settings::{GAMMA_RANGE, Settings, fields};
use crate::ui::components::{Segmented, Slider, Switch};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::VIDEO_COLOR;

pub(super) struct VideoPane;

impl Pane for VideoPane {
    fn name(&self) -> &'static str {
        "Video"
    }

    fn icon(&self) -> &'static str {
        "display"
    }

    fn color(&self) -> u32 {
        VIDEO_COLOR
    }

    fn content(
        &self,
        view: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        _: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let [sdr_gamma, hdr_gamma] = s.gamma();
        vec![
            group(
                Some("Picture"),
                theme,
                vec![
                    row(
                        "Scaling",
                        None,
                        theme,
                        Segmented::new("fit", &fields::FITS, s.video_fit)
                            .on_select(|&v, _, cx| set("video_fit", v, cx)),
                    ),
                    row(
                        "SDR gamma",
                        Some(
                            "Above 1 brightens shadows and midtones, below 1 darkens them. For \
                             every open video."
                                .into(),
                        ),
                        theme,
                        gamma_slider(view, "sdr-gamma", "sdr_gamma", sdr_gamma, theme),
                    ),
                    row(
                        "HDR gamma",
                        Some("The same for HDR10, HLG and Dolby Vision video.".into()),
                        theme,
                        gamma_slider(view, "hdr-gamma", "hdr_gamma", hdr_gamma, theme),
                    ),
                ],
            ),
            group(
                Some("Decoding"),
                theme,
                vec![
                    row(
                        "Resolution limit",
                        Some(
                            "For files opened from now on. Lower limits save CPU and memory \
                             with many videos."
                                .into(),
                        ),
                        theme,
                        Segmented::new("max-resolution", &fields::RESOLUTIONS, s.max_resolution)
                            .on_select(|&v, _, cx| set("max_resolution", v, cx)),
                    ),
                    row(
                        "Hardware decoding",
                        Some(
                            if cfg!(target_os = "macos") {
                                "Decode on the GPU with VideoToolbox when it supports the \
                                 video. For files opened from now on."
                            } else {
                                "Decode on the GPU with VA-API, NVDEC or VDPAU when one \
                                 supports the video. For files opened from now on."
                            }
                            .into(),
                        ),
                        theme,
                        Switch::new("hardware", s.hardware_decoding)
                            .on_toggle(|&on, _, cx| set("hardware_decoding", on, cx)),
                    ),
                ],
            ),
        ]
    }
}

/// A gamma slider. Gamma is spread evenly by its logarithm, so 1 sits in the
/// middle, and moves in steps of 0.05, so it comes back to exactly 1.
fn gamma_slider(
    view: &mut SettingsView,
    id: &'static str,
    key: &'static str,
    value: f32,
    theme: &Theme,
) -> AnyElement {
    let (low, high) = (GAMMA_RANGE.start().log2(), GAMMA_RANGE.end().log2());
    let position = (value.log2() - low) / (high - low);
    labeled_slider(
        Slider::new(id, position, view.drag(id)).on_change(move |event, _, cx| {
            let gamma = (low + event.value * (high - low)).exp2();
            let gamma = (gamma * 20.0).round() / 20.0;
            slide(key, gamma, event.slide, cx);
        }),
        format!("{value:.2}"),
        theme,
    )
}
