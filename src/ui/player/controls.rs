//! The controls floating over the bottom of a tile: the seek bar, and under
//! it the time, in the middle the previous file, skip back, play, skip
//! forward and the next file, and tracks, loop and volume.

use gpui::prelude::*;
use gpui::{ClickEvent, Context, Corner, Div, Pixels, Window, div, px};

use super::tracks::{close_tracks, has_track_choice};
use super::{PlayerView, Reveal};
use crate::media::chapters::{self, Chapter};
use crate::media::decoder::MediaInfo;
use crate::settings::Settings;
use crate::ui::components::{
    Badge, Button, ComponentSize, Panel, Popover, Sizable, Slide, Slider, SliderEvent,
};
use crate::ui::theme::player::{CONTROLS_GAP, CONTROLS_INSET, TRACK_MENU_OFFSET, VOLUME_WIDTH};
use crate::ui::theme::{Theme, controls, overlay, panel};

/// The controls' panel is small: it is short, and has buttons close to its
/// corners.
const PANEL_SIZE: ComponentSize = ComponentSize::Small;

/// How far up the tile the controls reach: the seek bar and the buttons in
/// the panel's padding, and the gap below it. Subtitles stay above them
/// while they show.
pub(super) fn height(live: bool) -> Pixels {
    CONTROLS_INSET
        + PANEL_SIZE.pick(panel::OVERLAY_PADDING) * 2.
        + ComponentSize::Medium.pick(controls::HEIGHT)
        + if live {
            px(0.)
        } else {
            ComponentSize::Medium.pick(controls::SLIDER_HEIGHT) + CONTROLS_GAP
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
        let duration = self.duration();
        let position = self.scrub.unwrap_or_else(|| self.position());
        let progress = duration
            .filter(|d| *d > 0.0)
            .map_or(0.0, |d| (position / d) as f32);
        let chapters = info.map_or(&[][..], |info| info.chapters.as_slice());
        let time_label = time_label(position, duration, chapters);
        let notches = match duration {
            Some(d) if cx.global::<Settings>().chapter_marks => chapters::marks(chapters, d),
            _ => Vec::new(),
        };
        let play_icon = if self.is_playing() {
            "pause"
        } else if self.has_ended() {
            "replay"
        } else {
            "play"
        };
        let player = self.playback.local();
        let volume_icon = if player.is_muted() || player.volume() == 0.0 {
            "mute"
        } else {
            "volume"
        };
        let has_audio = info.is_some_and(|i| i.has_audio);
        let cast = self.render_cast_button(info, theme, window, cx);
        let tracks = info.filter(|info| has_track_choice(info));
        let subtitles_on = info.is_some_and(|i| i.subtitle_track.is_some());
        let looping = player.is_looping();

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
                    .with_size(PANEL_SIZE)
                    .flex()
                    .flex_col()
                    .gap(CONTROLS_GAP)
                    .when(!live, |this| {
                        this.child(
                            div().px_1().debug_selector(|| "seek".into()).child(
                                Slider::new("seek", progress, self.seek_drag.clone())
                                    .overlay()
                                    .fill(theme.accent)
                                    .notches(notches)
                                    .on_change(cx.listener(Self::on_seek)),
                            ),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_0p5()
                            // The time and the buttons on the right take equal
                            // room, so the transport is in the middle.
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .pl_1()
                                    .when(live, |this| {
                                        this.child(Badge::label("LIVE", theme.danger))
                                    })
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_xs()
                                            .text_color(overlay::text())
                                            .child(time_label),
                                    ),
                            )
                            .child(self.render_transport(live, play_icon, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap_0p5()
                                    .children(cast)
                                    .when_some(tracks, |this, info| {
                                        let button = Button::icon("tracks", "subtitles")
                                            .overlay()
                                            .selected(subtitles_on)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.tracks_open = !this.tracks_open;
                                                cx.notify();
                                            }));
                                        this.child(
                                            Popover::new("tracks-backdrop", button)
                                                .anchor(Corner::BottomRight, TRACK_MENU_OFFSET)
                                                .when(self.tracks_open, |this| {
                                                    this.menu(
                                                        self.render_tracks_menu(info, theme, cx),
                                                    )
                                                })
                                                .on_dismiss(cx.listener(close_tracks)),
                                        )
                                    })
                                    .when(!live, |this| {
                                        this.child(
                                            Button::icon("loop", "loop")
                                                .overlay()
                                                .icon_color(overlay::text_muted())
                                                .selected(looping)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    let looping =
                                                        !this.playback.local().is_looping();
                                                    this.set_looping(looping);
                                                    cx.notify();
                                                })),
                                        )
                                    })
                                    .when(has_audio, |this| {
                                        this.child(
                                            Button::icon("mute", volume_icon).overlay().on_click(
                                                cx.listener(|this, _, _, cx| {
                                                    let muted = !this.playback.local().is_muted();
                                                    this.set_muted(muted);
                                                    cx.notify();
                                                }),
                                            ),
                                        )
                                        .child(
                                            div().w(VOLUME_WIDTH).pr_1().child(
                                                Slider::new(
                                                    "volume",
                                                    if player.is_muted() {
                                                        0.0
                                                    } else {
                                                        player.volume()
                                                    },
                                                    self.volume_drag.clone(),
                                                )
                                                .overlay()
                                                .on_change(cx.listener(Self::on_volume)),
                                            ),
                                        )
                                    }),
                            ),
                    ),
            )
    }

    /// The file before this one in its folder, skip back, play, skip
    /// forward, and the file after. Skips go by the seek step in Settings,
    /// as the arrow keys do; a live stream can't skip.
    fn render_transport(&self, live: bool, play_icon: &'static str, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .child(
                Button::icon("previous", "previous")
                    .overlay()
                    .disabled(
                        self.neighbors
                            .as_ref()
                            .is_none_or(|neighbors| neighbors.previous.is_none()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.open_neighbor(-1, cx))),
            )
            .child(
                Button::icon("skip-back", "skip-back")
                    .overlay()
                    .disabled(live)
                    .on_click(cx.listener(skip(-1.0))),
            )
            .child(
                Button::icon("play", play_icon)
                    .overlay()
                    .on_click(cx.listener(|this, _, _, cx| this.play_pause(cx))),
            )
            .child(
                Button::icon("skip-forward", "skip-forward")
                    .overlay()
                    .disabled(live)
                    .on_click(cx.listener(skip(1.0))),
            )
            .child(
                Button::icon("next", "next")
                    .overlay()
                    .disabled(
                        self.neighbors
                            .as_ref()
                            .is_none_or(|neighbors| neighbors.next.is_none()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.open_neighbor(1, cx))),
            )
    }

    fn on_seek(&mut self, event: &SliderEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(duration) = self.duration() else {
            return;
        };
        let time = event.value as f64 * duration;
        // The bar shows where the slider is until it's let go, wherever it
        // plays.
        self.scrub = (event.slide != Slide::End).then_some(time);
        self.playback.scrub(event.slide, time);
        cx.notify();
    }

    fn on_volume(&mut self, event: &SliderEvent, _: &mut Window, cx: &mut Context<Self>) {
        let value = event.value;
        self.set_volume(value);
        if value > 0.0 && self.playback.local().is_muted() {
            self.set_muted(false);
        }
        cx.notify();
    }
}

/// Skip back (`-1`) or forward (`1`) by the seek step.
fn skip(
    direction: f64,
) -> impl Fn(&mut PlayerView, &ClickEvent, &mut Window, &mut Context<PlayerView>) + 'static {
    move |this, _, _, cx| {
        let step = f64::from(cx.global::<Settings>().seek_step_seconds);
        this.seek_by(step * direction, cx);
    }
}

/// Where it is, of how long it is, and the chapter it's in for a file of
/// several.
fn time_label(position: f64, duration: Option<f64>, chapters: &[Chapter]) -> String {
    let time = match duration {
        Some(d) => format!("{} / {}", format_time(position), format_time(d)),
        None => format_time(position),
    };
    match chapters::at(chapters, position).filter(|_| chapters.len() > 1) {
        Some(index) => format!("{time} · {}", chapters[index].name(index)),
        None => time,
    }
}

pub(super) fn format_time(seconds: f64) -> String {
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
    use super::{Chapter, format_time, time_label};

    #[test]
    fn the_time_says_which_chapter_it_is_in() {
        let chapter = |start: f64, title: &str| Chapter {
            start,
            end: start + 60.0,
            title: Some(title.to_owned()),
        };
        let chapters = [chapter(0.0, "Opening"), chapter(60.0, "Middle")];
        assert_eq!(
            time_label(75.0, Some(120.0), &chapters),
            "1:15 / 2:00 · Middle"
        );
        assert_eq!(
            time_label(75.0, Some(120.0), &chapters[..1]),
            "1:15 / 2:00",
            "not for a file of one part"
        );
        assert_eq!(time_label(5.0, None, &[]), "0:05");
    }

    #[test]
    fn formats_times() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(59.9), "0:59");
        assert_eq!(format_time(61.0), "1:01");
        assert_eq!(format_time(3725.0), "1:02:05");
        assert_eq!(format_time(-3.0), "0:00");
    }
}
