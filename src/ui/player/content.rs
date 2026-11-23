//! What fills a block: the video; for audio, cover art, the file's tags and
//! quality, and a level meter, over a visualizer, as Settings say; or a
//! line on what is happening (opening, an error).

use gpui::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Bounds, ContentTransform, Corners, ObjectFit, Pixels, SharedString, canvas,
    div, img, relative,
};

use super::sound::SoundLines;
use super::visualizer::speaker;
use super::{PlayerView, controls};
use crate::media::Picture;
use crate::media::decoder::MediaInfo;
use crate::settings::{AudioMode, Settings, Visualizer};
use crate::ui::components::{ComponentSize, Icon, Meter};
use crate::ui::framing::Framing;
use crate::ui::theme::player::{
    COVER_MIN_SIZE, ERROR_MAX_WIDTH, METER_WIDTH, NO_COVER_ICON_SCALE, NO_COVER_SCALE, NOTE_TOP,
    PIP_ICON, SPEAKER_OPACITY,
};
use crate::ui::theme::{Theme, radius};

impl PlayerView {
    /// `framing` is how the video shows, `cover` whether it fills the block.
    pub(super) fn render_content(
        &self,
        theme: &Theme,
        framing: Option<Framing>,
        cover: bool,
        cx: &App,
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
            Some(info) if !info.has_video => {
                self.render_audio(theme, &info, cx.global::<Settings>().audio_mode)
            }
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

    /// Audio-only files get their cover art (or a circle standing in for
    /// it), their tags, their quality if `mode` says, and a level meter,
    /// over the visualizer it picks: a speaker whose cone moves with the
    /// level, or none.
    fn render_audio(&self, theme: &Theme, info: &MediaInfo, mode: AudioMode) -> AnyElement {
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
            .pt(NOTE_TOP)
            .pb(controls::height(info.live))
            .children(match mode.visualizer {
                Visualizer::Speaker => {
                    Some(speaker(self.level, theme.accent.opacity(SPEAKER_OPACITY)))
                }
                Visualizer::None => None,
            })
            .child(self.render_cover(theme))
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
                    .children(
                        quality
                            .filter(|_| mode.show_quality)
                            .map(|quality| line("sound-quality", quality)),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| "sound-meter".into())
                    .w(METER_WIDTH)
                    .max_w_full()
                    .child(Meter::new(self.level)),
            )
            .into_any_element()
    }

    /// The cover art, or a circle standing in for it, square, and as large
    /// as fits with the rest under it, between where the header and the
    /// controls go.
    fn render_cover(&self, theme: &Theme) -> impl IntoElement {
        let artwork = match self.playback.local().cover() {
            Some(cover) => img(cover)
                .debug_selector(|| "sound-cover-art".into())
                .size_full()
                .object_fit(ObjectFit::Contain)
                .superellipse(ComponentSize::Medium.pick(radius::MENU))
                .into_any_element(),
            None => div()
                .debug_selector(|| "sound-no-cover".into())
                .flex()
                .items_center()
                .justify_center()
                .size(relative(NO_COVER_SCALE))
                .rounded_full()
                .bg(theme.surface_hover)
                .child(
                    Icon::new("music")
                        .size(relative(NO_COVER_ICON_SCALE))
                        .color(theme.accent)
                        .debug_selector("sound-no-cover-icon"),
                )
                .into_any_element(),
        };
        // It grows into the room the rest leaves, no wider than the block.
        // Square: its least and most widths make its least and most heights,
        // and something as wide as it is tall in it makes it as wide.
        div()
            .debug_selector(|| "sound-cover".into())
            .relative()
            .flex()
            .flex_1()
            .min_h(COVER_MIN_SIZE)
            .max_w_full()
            .map(square)
            .child(div().h_full().map(square))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(artwork),
            )
    }
}

/// `element` as wide as it is tall.
fn square<E: Styled>(mut element: E) -> E {
    element.style().aspect_ratio = Some(1.);
    element
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
