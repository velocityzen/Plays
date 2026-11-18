//! The controls floating over the bottom of a tile: the seek bar, and under
//! it play, the time, tracks, loop and volume.

use gpui::prelude::*;
use gpui::{
    AnchoredPositionMode, Context, Corner, Div, FontWeight, MouseButton, Pixels, Window, anchored,
    deferred, div, point, px,
};

use super::tracks::{close_tracks, has_track_choice};
use super::{PlayerView, Reveal};
use crate::media::decoder::MediaInfo;
use crate::ui::components::{IconButton, Panel, Slide, Slider, SliderEvent};
use crate::ui::theme::player::{
    CONTROLS_GAP, CONTROLS_INSET, CONTROLS_PADDING, TRACK_MENU_OFFSET, VOLUME_WIDTH,
};
use crate::ui::theme::{Theme, controls, menu, overlay, radius};

/// How far up the tile the controls reach: the seek bar and the buttons in
/// the panel's padding, and the gap below it. Subtitles stay above them
/// while they show.
pub(super) fn height(live: bool) -> Pixels {
    CONTROLS_INSET
        + CONTROLS_PADDING * 2.
        + controls::ICON_BUTTON
        + if live {
            px(0.)
        } else {
            controls::SLIDER_HEIGHT + CONTROLS_GAP
        }
}

impl PlayerView {
    pub(super) fn render_controls(
        &self,
        reveal: &Reveal,
        info: Option<&MediaInfo>,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let live = info.is_some_and(|i| i.live);
        let duration = self.player.duration();
        let position = self.scrub.unwrap_or_else(|| self.player.position());
        let progress = duration
            .filter(|d| *d > 0.0)
            .map_or(0.0, |d| (position / d) as f32);
        let time_label = match duration {
            Some(d) => format!("{} / {}", format_time(position), format_time(d)),
            None => format_time(position),
        };
        let play_icon = if self.player.is_playing() {
            "pause"
        } else if duration.is_some_and(|d| position >= d - 0.05) {
            "replay"
        } else {
            "play"
        };
        let volume_icon = if self.player.is_muted() || self.player.volume() == 0.0 {
            "mute"
        } else {
            "volume"
        };
        let has_audio = info.is_some_and(|i| i.has_audio);
        let tracks = info.filter(|info| has_track_choice(info));
        let subtitles_on = info.is_some_and(|i| i.subtitle_track.is_some());
        let looping = self.player.is_looping();

        // The controls float on a panel over the bottom of the tile. Its
        // corners are less round than other panels', as it is short and
        // buttons sit close to them.
        reveal
            .apply(
                div()
                    .absolute()
                    .bottom(CONTROLS_INSET)
                    .left(CONTROLS_INSET)
                    .right(CONTROLS_INSET),
            )
            .child(
                Panel::new()
                    .overlay()
                    .superellipse(radius::CONTROLS)
                    .flex()
                    .flex_col()
                    .gap(CONTROLS_GAP)
                    .p(CONTROLS_PADDING)
                    .when(!live, |this| {
                        this.child(
                            div().px_1().child(
                                Slider::new("seek", progress, self.seek_drag.clone())
                                    .overlay()
                                    .fill(theme.accent)
                                    .on_change(cx.listener(Self::on_seek)),
                            ),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_0p5()
                            .child(IconButton::new("play", play_icon).overlay().on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.player.toggle();
                                    cx.notify();
                                }),
                            ))
                            .when(live, |this| {
                                this.child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .ml_1()
                                        .px_1p5()
                                        .superellipse(radius::BADGE)
                                        .bg(theme.danger)
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(overlay::text())
                                        .child("LIVE"),
                                )
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .pl_1()
                                    .text_xs()
                                    .text_color(overlay::text())
                                    .child(time_label),
                            )
                            .child(div().flex_1())
                            .when_some(tracks, |this, info| {
                                let button = IconButton::new("tracks", "subtitles")
                                    .overlay()
                                    .when(subtitles_on, |this| this.color(theme.accent))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.tracks_open = !this.tracks_open;
                                        cx.notify();
                                    }));
                                this.child(div().relative().child(button).when(
                                    self.tracks_open,
                                    |this| {
                                        this.child(
                                            deferred(
                                                anchored()
                                                    .position_mode(AnchoredPositionMode::Local)
                                                    .anchor(Corner::BottomRight)
                                                    .position(TRACK_MENU_OFFSET)
                                                    .snap_to_window_with_margin(menu::WINDOW_MARGIN)
                                                    .child(
                                                        self.render_tracks_menu(info, theme, cx),
                                                    ),
                                            )
                                            .with_priority(2),
                                        )
                                        // Clicks anywhere else close the menu, and do
                                        // nothing else.
                                        .child(
                                            deferred(
                                                anchored().position(point(px(0.), px(0.))).child(
                                                    div()
                                                        .id("tracks-backdrop")
                                                        .occlude()
                                                        .w(window.viewport_size().width)
                                                        .h(window.viewport_size().height)
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(close_tracks),
                                                        )
                                                        .on_mouse_down(
                                                            MouseButton::Right,
                                                            cx.listener(close_tracks),
                                                        ),
                                                ),
                                            )
                                            .with_priority(1),
                                        )
                                    },
                                ))
                            })
                            .when(!live, |this| {
                                this.child(
                                    IconButton::new("loop", "loop")
                                        .overlay()
                                        .color(if looping {
                                            theme.accent
                                        } else {
                                            overlay::text_muted()
                                        })
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            let looping = !this.player.is_looping();
                                            this.player.set_looping(looping);
                                            cx.notify();
                                        })),
                                )
                            })
                            .when(has_audio, |this| {
                                this.child(IconButton::new("mute", volume_icon).overlay().on_click(
                                    cx.listener(|this, _, _, cx| {
                                        let muted = !this.player.is_muted();
                                        this.player.set_muted(muted);
                                        cx.notify();
                                    }),
                                ))
                                .child(
                                    div().w(VOLUME_WIDTH).pr_1().child(
                                        Slider::new(
                                            "volume",
                                            if self.player.is_muted() {
                                                0.0
                                            } else {
                                                self.player.volume()
                                            },
                                            self.volume_drag.clone(),
                                        )
                                        .overlay()
                                        .on_change(cx.listener(Self::on_volume)),
                                    ),
                                )
                            }),
                    ),
            )
    }

    fn on_seek(&mut self, event: &SliderEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(duration) = self.player.duration() else {
            return;
        };
        let time = event.value as f64 * duration;
        match event.slide {
            Slide::Start => {
                self.resume_after_scrub = self.player.is_playing();
                self.player.pause();
                self.scrub = Some(time);
                self.player.seek(time);
            }
            Slide::Move => {
                self.scrub = Some(time);
                self.player.seek(time);
            }
            Slide::End => {
                self.scrub = None;
                self.player.seek(time);
                if self.resume_after_scrub {
                    self.player.play();
                }
            }
        }
        cx.notify();
    }

    fn on_volume(&mut self, event: &SliderEvent, _: &mut Window, cx: &mut Context<Self>) {
        let value = event.value;
        self.player.set_volume(value);
        if value > 0.0 && self.player.is_muted() {
            self.player.set_muted(false);
        }
        cx.notify();
    }
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_time;

    #[test]
    fn formats_times() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(59.9), "0:59");
        assert_eq!(format_time(61.0), "1:01");
        assert_eq!(format_time(3725.0), "1:02:05");
        assert_eq!(format_time(-3.0), "0:00");
    }
}
