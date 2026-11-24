//! What the screenshot shortcut and menu item do to a block: save the frame
//! it shows, and say where it went.

use std::path::Path;

use gpui::{AppContext as _, Context, Window};

use super::PlayerView;
use crate::settings::Settings;
use crate::ui::overlay;
use crate::ui::screenshot::{self, Request};
use crate::ui::theme::Theme;

impl PlayerView {
    /// Save the frame showing as a file, in the background, and say where
    /// it went once it's there.
    pub fn take_screenshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(info) = self.playback.local().info().filter(|info| info.has_video) else {
            self.note("No video", cx);
            return;
        };
        if info.live {
            self.note("Live streams can't be screenshotted", cx);
            return;
        }
        let (Some(stream), Some(timestamp), Some(position)) = (
            info.video_track,
            self.playback.local().frame_timestamp(),
            self.playback.local().frame_pts(),
        ) else {
            self.note("No picture yet", cx);
            return;
        };
        let settings = cx.global::<Settings>();
        let shots = settings.screenshots.clone();
        let Some(folder) = shots.folder() else {
            self.note("No folder to save screenshots in", cx);
            return;
        };
        let cues = if shots.subtitles {
            self.cues.clone()
        } else {
            Vec::new()
        };
        let bitmaps = cues
            .iter()
            .flat_map(|cue| &cue.bitmaps)
            .map(|bitmap| (bitmap.image.clone(), bitmap.rect))
            .collect();
        let style = settings.subtitles.clone();
        let font = Theme::get(window, cx).font;
        let text = self.framing().map_or_else(Vec::new, |framing| {
            overlay::text_overlay(&cues, &style, &font, framing.size, window)
        });
        let request = Request {
            source: self.playback.local().source().clone(),
            max_short_side: self.playback.local().max_short_side(),
            stream,
            timestamp,
            position,
            orientation: info.orientation,
            adjustments: self.adjustments,
            bitmaps,
            text,
            text_system: cx.text_system().clone(),
            settings: shots,
            folder,
        };
        let task = cx.background_spawn(async move { screenshot::take(request) });
        cx.spawn(async move |this, cx| {
            let taken = task.await;
            this.update(cx, |this, cx| match taken {
                Ok(path) => {
                    log::info!("saved a screenshot to {}", path.display());
                    this.note_change(saved(&path), cx);
                }
                Err(err) => {
                    log::warn!("screenshot failed: {err:#}");
                    this.note(format!("Screenshot failed: {err}"), cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

/// Where a screenshot went, as a note says it.
fn saved(path: &Path) -> String {
    match path.parent().and_then(Path::file_name) {
        Some(folder) => format!("Screenshot saved to {}", folder.to_string_lossy()),
        None => "Screenshot saved".to_owned(),
    }
}
