//! The controls floating over the bottom of a tile: the seek bar, and under
//! it the time, in the middle the previous file, skip back, play, skip
//! forward and the next file, and tracks, loop and volume.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Corner, Div, Pixels, Window, div, px};
use serde_json::{Value, json};

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

/// How far up the tile subtitles keep clear of, for the controls: none
/// when they're hidden, as they're never drawn.
pub(super) fn clearance(shown: bool, live: bool) -> Pixels {
    if shown { height(live) } else { px(0.) }
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
        let position = self.bar_position();
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
        let cast = self.render_cast_button(info, theme, window, cx);

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
                        this.child(self.render_seek_bar(progress, notches, theme, cx))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_0p5()
                            // The time and the buttons on the right take equal
                            // room, so the transport is in the middle.
                            .child(render_time(live, time_label, theme))
                            .child(self.render_transport(live, play_icon, cx))
                            .child(self.render_buttons(live, info, cast, theme, cx)),
                    ),
            )
    }

    /// The seek bar, with a notch where each chapter starts.
    fn render_seek_bar(
        &self,
        progress: f32,
        notches: Vec<f32>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        div().px_1().debug_selector(|| "seek".into()).child(
            Slider::new("seek", progress, self.seek_drag.clone())
                .overlay()
                .fill(theme.accent)
                .notches(notches)
                .on_change(cx.listener(Self::on_seek)),
        )
    }

    /// The buttons on the right: casting, tracks, loop, and the volume.
    fn render_buttons(
        &self,
        live: bool,
        info: Option<&MediaInfo>,
        cast: Option<AnyElement>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        let player = self.playback.local();
        let volume_icon = if player.is_muted() || player.volume() == 0.0 {
            "mute"
        } else {
            "volume"
        };
        let has_audio = info.is_some_and(|i| i.has_audio);
        let tracks = info.filter(|info| has_track_choice(info));
        let looping = player.is_looping();
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_end()
            .gap_0p5()
            .children(cast)
            .when_some(tracks, |this, info| {
                this.child(self.render_tracks_button(info, theme, cx))
            })
            .when(!live, |this| {
                this.child(
                    Button::icon("loop", "loop")
                        .overlay()
                        .icon_color(overlay::text_muted())
                        .selected(looping)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.send("playback.toggle_loop", Value::Null, cx);
                        })),
                )
            })
            .when(has_audio, |this| {
                this.child(
                    Button::icon("mute", volume_icon)
                        .overlay()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.send("audio.toggle_mute", Value::Null, cx);
                        })),
                )
                .child(
                    div()
                        .w(VOLUME_WIDTH)
                        .pr_1()
                        .debug_selector(|| "volume".into())
                        .child(
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
            })
    }

    /// The tracks button, lit while subtitles show, and the menu of tracks
    /// it opens.
    fn render_tracks_button(
        &self,
        info: &MediaInfo,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Popover {
        let subtitles_on = info.subtitle_track.is_some();
        let button = Button::icon("tracks", "subtitles")
            .overlay()
            .selected(subtitles_on)
            .on_click(cx.listener(|this, _, _, cx| {
                this.tracks_open = !this.tracks_open;
                cx.notify();
            }));
        Popover::new("tracks-backdrop", button)
            .anchor(Corner::BottomRight, TRACK_MENU_OFFSET)
            .when(self.tracks_open, |this| {
                this.menu(self.render_tracks_menu(info, theme, cx))
            })
            .on_dismiss(cx.listener(close_tracks))
    }

    /// The file before this one in its playlist or folder, skip back,
    /// play, skip forward, and the file after. Skips go by the seek step in Settings,
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
                    .disabled(!self.has_beside(-1))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send("playback.previous_file", Value::Null, cx);
                    })),
            )
            .child(
                Button::icon("skip-back", "skip-back")
                    .overlay()
                    .disabled(live)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send("playback.seek_backward", Value::Null, cx);
                    })),
            )
            .child(
                Button::icon("play", play_icon)
                    .overlay()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send("playback.play_pause", Value::Null, cx);
                    })),
            )
            .child(
                Button::icon("skip-forward", "skip-forward")
                    .overlay()
                    .disabled(live)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send("playback.seek_forward", Value::Null, cx);
                    })),
            )
            .child(
                Button::icon("next", "next")
                    .overlay()
                    .disabled(!self.has_beside(1))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send("playback.next_file", Value::Null, cx);
                    })),
            )
    }

    fn on_seek(&mut self, event: &SliderEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(duration) = self.duration() else {
            return;
        };
        let time = f64::from(event.value) * duration;
        // The bar shows where the slider is until it plays from there,
        // wherever it plays: dragged, it follows here; let go, it seeks.
        self.scrub = Some(time);
        match event.slide {
            Slide::Start => self.playback.begin_scrub(time),
            Slide::Move => self.playback.scrub(time),
            Slide::End => self.send("playback.seek", json!({"time": time}), cx),
        }
        cx.notify();
    }

    /// Dragged, the volume follows; let go, it's set.
    fn on_volume(&mut self, event: &SliderEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.slide {
            Slide::End => {
                let volume = f64::from(event.value) * 100.0;
                self.send("audio.set_volume", json!({"volume": volume}), cx);
            }
            _ => self.set_volume_to(event.value, cx),
        }
    }
}

/// The time on the left, after a LIVE badge for a live stream.
fn render_time(live: bool, time_label: String, theme: &Theme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap_1()
        .pl_1()
        .when(live, |this| this.child(Badge::label("LIVE", theme.danger)))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_xs()
                .text_color(overlay::text())
                .child(time_label),
        )
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

pub fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0).floor() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
#[path = "tests/controls.rs"]
mod tests;
