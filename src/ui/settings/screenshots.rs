//! Screenshots: where they go, in what format, and what goes in them.

use std::path::Path;

use gpui::prelude::*;
use gpui::{AnyElement, App, Context, PathPromptOptions, SharedString, Window, div};

use super::form::{group, labeled_slider, row, update_settings};
use super::{Pane, SettingsView};
use crate::settings::{
    JPEG_QUALITY_RANGE, ScreenshotFormat, ScreenshotHdr, ScreenshotSettings, Settings,
};
use crate::ui::components::{Button, Segmented, Slide, Slider, Switch};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::SCREENSHOTS_COLOR;

pub(super) struct ScreenshotsPane;

impl Pane for ScreenshotsPane {
    fn name(&self) -> &'static str {
        "Screenshots"
    }

    fn icon(&self) -> &'static str {
        "camera"
    }

    fn color(&self) -> u32 {
        SCREENSHOTS_COLOR
    }

    fn content(
        &self,
        view: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        _: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let shots = &s.screenshots;
        let jpeg = shots.format == ScreenshotFormat::Jpeg;
        let mut saving = vec![
            row(
                "Folder",
                Some(folder_text(shots)),
                theme,
                folder_buttons(shots),
            ),
            row(
                "Format",
                None,
                theme,
                Segmented::new(
                    "screenshot-format",
                    &[
                        (ScreenshotFormat::Png, "PNG"),
                        (ScreenshotFormat::Jpeg, "JPEG"),
                    ],
                    shots.format,
                )
                .on_select(|&v, _, cx| update_settings(cx, true, |s| s.screenshots.format = v)),
            ),
        ];
        if jpeg {
            saving.push(row(
                "JPEG quality",
                Some("Higher keeps more detail in bigger files.".into()),
                theme,
                quality_slider(view, shots.jpeg_quality(), theme),
            ));
        }
        vec![
            group(Some("Saving"), theme, saving),
            group(
                Some("Picture"),
                theme,
                vec![
                    row(
                        "Subtitles",
                        Some("Draw in the subtitles showing.".into()),
                        theme,
                        Switch::new("screenshot-subtitles", shots.subtitles).on_toggle(
                            |&on, _, cx| {
                                update_settings(cx, true, |s| s.screenshots.subtitles = on)
                            },
                        ),
                    ),
                    row(
                        "HDR video",
                        Some(
                            if jpeg {
                                "JPEGs are always SDR: HDR10, HLG and Dolby Vision are saved as \
                                 an SDR screen shows them."
                            } else {
                                "SDR saves HDR10, HLG and Dolby Vision as an SDR screen shows \
                                 them. HDR keeps them as they are, in a 16-bit PNG that only \
                                 HDR-aware apps show right."
                            }
                            .into(),
                        ),
                        theme,
                        Segmented::new(
                            "screenshot-hdr",
                            &[(ScreenshotHdr::Sdr, "SDR"), (ScreenshotHdr::Keep, "HDR")],
                            if jpeg { ScreenshotHdr::Sdr } else { shots.hdr },
                        )
                        .disabled(jpeg)
                        .on_select(|&v, _, cx| {
                            update_settings(cx, true, |s| s.screenshots.hdr = v)
                        }),
                    ),
                ],
            ),
        ]
    }
}

/// The folder screenshots go to, home shortened to `~`.
fn folder_text(shots: &ScreenshotSettings) -> SharedString {
    let Some(folder) = shots.folder() else {
        return "This system has no folder for them".into();
    };
    let home = dirs::home_dir();
    let shown = match home
        .as_deref()
        .and_then(|home| folder.strip_prefix(home).ok())
    {
        Some(rest) if rest == Path::new("") => "~".to_owned(),
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => folder.display().to_string(),
    };
    shown.into()
}

fn folder_buttons(shots: &ScreenshotSettings) -> impl IntoElement {
    let folder = shots.folder();
    let chosen = shots.folder.is_some();
    div()
        .flex()
        .flex_none()
        .gap_2()
        .when_some(folder, |this, folder| {
            this.child(
                Button::new("screenshot-folder-show", "Show").on_click(move |_, _, cx| {
                    if let Err(err) = std::fs::create_dir_all(&folder) {
                        log::warn!("can't make {}: {err}", folder.display());
                    }
                    cx.open_with_system(&folder);
                }),
            )
        })
        .when(chosen, |this| {
            this.child(
                Button::new("screenshot-folder-default", "Use Pictures").on_click(|_, _, cx| {
                    update_settings(cx, true, |s| s.screenshots.folder = None)
                }),
            )
        })
        .child(
            Button::new("screenshot-folder-choose", "Choose…")
                .on_click(|_, _, cx| choose_folder(cx)),
        )
}

/// Ask for a folder with the system's dialog, and save screenshots there.
fn choose_folder(cx: &mut App) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Choose".into()),
    });
    cx.spawn(async move |cx| {
        if let Ok(Ok(Some(paths))) = picked.await
            && let Some(folder) = paths.into_iter().next()
        {
            cx.update(|cx| update_settings(cx, true, |s| s.screenshots.folder = Some(folder)))
                .ok();
        }
    })
    .detach();
}

/// JPEG quality, in steps of 1.
fn quality_slider(view: &mut SettingsView, value: u8, theme: &Theme) -> AnyElement {
    let id = "jpeg-quality";
    let (low, high) = (
        f32::from(*JPEG_QUALITY_RANGE.start()),
        f32::from(*JPEG_QUALITY_RANGE.end()),
    );
    let position = (f32::from(value) - low) / (high - low);
    labeled_slider(
        Slider::new(id, position, view.drag(id)).on_change(move |event, _, cx| {
            let quality = (low + event.value * (high - low)).round() as u8;
            update_settings(cx, event.slide == Slide::End, |s| {
                s.screenshots.jpeg_quality = quality
            });
        }),
        value.to_string(),
        theme,
    )
}
