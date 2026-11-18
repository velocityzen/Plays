//! What fills a tile: the video; cover art and a level meter for audio; or
//! a line on what is happening (opening, an error).

use gpui::prelude::*;
use gpui::{AnyElement, ContentTransform, Corners, ObjectFit, SharedString, canvas, div, img};

use super::PlayerView;
use crate::media::Picture;
use crate::ui::components::{ComponentSize, Icon, Meter};
use crate::ui::framing::Framing;
use crate::ui::theme::player::{COVER_SIZE, ERROR_MAX_WIDTH, NO_COVER_ICON, NO_COVER_SIZE};
use crate::ui::theme::{Theme, radius};

impl PlayerView {
    /// `framing` is how the video shows, `cover` whether it fills the tile.
    pub(super) fn render_content(
        &self,
        theme: &Theme,
        framing: Option<Framing>,
        cover: bool,
    ) -> AnyElement {
        if let Some(error) = self.player.error() {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .max_w(ERROR_MAX_WIDTH)
                .px_4()
                .text_center()
                .child(div().text_sm().text_color(theme.danger).child(
                    if self.player.is_network() {
                        "Can't play this stream"
                    } else {
                        "Can't play this file"
                    },
                ))
                .child(div().text_xs().text_color(theme.text_muted).child(error))
                .into_any_element();
        }
        if let Some(frame) = self.player.frame() {
            return picture(frame, framing, cover);
        }
        match self.player.info() {
            Some(info) if !info.has_video => self.render_audio(theme),
            _ => div()
                .text_sm()
                .text_color(theme.text_muted)
                .child(if self.player.is_network() {
                    "Connecting…"
                } else {
                    "Opening…"
                })
                .into_any_element(),
        }
    }

    /// Audio-only files get their cover art (or a note) and a level meter.
    fn render_audio(&self, theme: &Theme) -> AnyElement {
        let artwork = match self.player.cover() {
            Some(cover) => img(cover)
                .size(COVER_SIZE)
                .object_fit(ObjectFit::Contain)
                .superellipse(ComponentSize::Medium.pick(radius::MENU))
                .into_any_element(),
            None => div()
                .flex()
                .items_center()
                .justify_center()
                .size(NO_COVER_SIZE)
                .rounded_full()
                .bg(theme.surface_hover)
                .child(Icon::new("music").size(NO_COVER_ICON).color(theme.accent))
                .into_any_element(),
        };
        let title = self
            .player
            .info()
            .and_then(|i| i.title)
            .map(SharedString::from)
            .unwrap_or_else(|| self.title.clone());
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .px_4()
            .child(artwork)
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_sm()
                    .text_color(theme.text)
                    .child(title),
            )
            .child(Meter::new(self.level).w(COVER_SIZE))
            .into_any_element()
    }
}

/// The frame, framed as `framing` says (as it is without), fitted in the
/// tile or covering it.
fn picture(frame: Picture, framing: Option<Framing>, cover: bool) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let (shown, content) = match framing {
                Some(framing) => (framing.fit(bounds, cover), framing.content()),
                None => (bounds, ContentTransform::IDENTITY),
            };
            match frame {
                Picture::Image(image) => {
                    let corners = Corners::default();
                    let _ = window
                        .paint_image_with_content(shown, corners, 0., image, 0, false, content);
                }
                #[cfg(target_os = "macos")]
                Picture::Surface(surface) => {
                    window.paint_surface_with_content(shown, surface.for_drawing(), content);
                }
                #[cfg(not(target_os = "macos"))]
                Picture::Yuv(yuv) => {
                    let gamma = crate::media::picture::gamma(yuv.hdr);
                    window.paint_yuv_frame(shown, yuv.frame, gamma, content);
                }
            }
        },
    )
    .size_full()
    .into_any_element()
}
