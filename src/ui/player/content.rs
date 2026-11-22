//! What fills a block: the video; for audio, cover art, the file's tags and
//! quality, and a level meter; or a line on what is happening (opening, an
//! error).

use gpui::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, ContentTransform, Corners, ObjectFit, Pixels, SharedString, canvas, div,
    img,
};

use super::PlayerView;
use super::sound::SoundLines;
use crate::media::Picture;
use crate::media::decoder::MediaInfo;
use crate::ui::components::{ComponentSize, Icon, Meter};
use crate::ui::framing::Framing;
use crate::ui::theme::player::{
    COVER_SIZE, ERROR_MAX_WIDTH, NO_COVER_ICON, NO_COVER_SIZE, PIP_ICON,
};
use crate::ui::theme::{Theme, radius};

impl PlayerView {
    /// `framing` is how the video shows, `cover` whether it fills the block.
    pub(super) fn render_content(
        &self,
        theme: &Theme,
        framing: Option<Framing>,
        cover: bool,
    ) -> AnyElement {
        if let Some(error) = self.playback.local().error() {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .max_w(ERROR_MAX_WIDTH)
                .px_4()
                .text_center()
                .child(div().text_sm().text_color(theme.danger).child(
                    if self.playback.local().is_network() {
                        "Can't play this stream"
                    } else {
                        "Can't play this file"
                    },
                ))
                .child(div().text_xs().text_color(theme.text_muted).child(error))
                .into_any_element();
        }
        if self.in_picture_in_picture() {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .child(Icon::new("pip").size(PIP_ICON).color(theme.text_muted))
                .into_any_element();
        }
        if let Some(frame) = self.playback.local().frame() {
            return picture(frame, framing, cover, self.picture_bounds.clone());
        }
        match self.playback.local().info() {
            Some(info) if !info.has_video => self.render_audio(theme, &info),
            _ => div()
                .text_sm()
                .text_color(theme.text_muted)
                .child(if self.playback.local().is_network() {
                    "Connecting…"
                } else {
                    "Opening…"
                })
                .into_any_element(),
        }
    }

    /// Audio-only files get their cover art (or a note), their tags and
    /// quality, and a level meter.
    fn render_audio(&self, theme: &Theme, info: &MediaInfo) -> AnyElement {
        let artwork = match self.playback.local().cover() {
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
        let SoundLines {
            title,
            artist,
            album,
            quality,
        } = SoundLines::of(info, &self.title);
        let line = |selector: &'static str, text: String| {
            div()
                .debug_selector(move || selector.into())
                .max_w_full()
                .truncate()
                .text_xs()
                .text_color(theme.text_muted)
                .child(SharedString::from(text))
        };
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
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_0p5()
                    .max_w_full()
                    .child(
                        div()
                            .debug_selector(|| "sound-title".into())
                            .max_w_full()
                            .truncate()
                            .text_sm()
                            .text_color(theme.text)
                            .child(title),
                    )
                    .children(
                        artist.map(|artist| line("sound-artist", artist).text_color(theme.text)),
                    )
                    .children(album.map(|album| line("sound-album", album)))
                    .children(quality.map(|quality| line("sound-quality", quality))),
            )
            .child(Meter::new(self.level).w(COVER_SIZE))
            .into_any_element()
    }
}

/// The frame, framed as `framing` says (as it is without), fitted in the
/// block or covering it. Where it goes is kept in `drawn_at`.
fn picture(
    frame: Picture,
    framing: Option<Framing>,
    cover: bool,
    drawn_at: Rc<Cell<Option<Bounds<Pixels>>>>,
) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let (shown, content) = match framing {
                Some(framing) => (framing.fit(bounds, cover), framing.content()),
                None => (bounds, ContentTransform::IDENTITY),
            };
            drawn_at.set(Some(bounds.intersect(&shown)));
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
