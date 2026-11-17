use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, EventEmitter, ImageId, MouseButton, ObjectFit, RenderImage,
    SharedString, Window, black, div, img, linear_color_stop, linear_gradient, px, relative,
};

use super::theme::{Theme, overlay};
use super::widgets::{Slide, SliderColors, format_time, icon, icon_button, slider};
use crate::media::{Mixer, Player, PlayerOptions, Source};
use crate::settings::{ControlsVisibility, Settings, VideoFit};

/// Frames that were on screen wait here for a few display frames before their
/// GPU textures are freed, so a frame still in flight on the GPU keeps its texture.
#[derive(Clone, Default)]
pub struct Graveyard(Rc<RefCell<Vec<Buried>>>);

/// A retired frame and when it went off screen.
type Buried = (Arc<RenderImage>, Instant);

const RELEASE_DELAY: Duration = Duration::from_millis(60);

impl Graveyard {
    pub fn bury(&self, image: Arc<RenderImage>) {
        self.0.borrow_mut().push((image, Instant::now()));
    }

    /// Free textures that have been off screen long enough. Returns whether any are left.
    pub fn collect(&self, window: &mut Window) -> bool {
        let mut images = self.0.borrow_mut();
        images.retain(|(image, buried)| {
            let expired = buried.elapsed() >= RELEASE_DELAY;
            if expired {
                let _ = window.drop_image(image.clone());
            }
            !expired
        });
        !images.is_empty()
    }
}

pub enum PlayerEvent {
    Close,
    ToggleMaximize,
}

pub struct PlayerView {
    player: Player,
    title: SharedString,
    graveyard: Graveyard,
    seek_drag: Rc<Cell<bool>>,
    volume_drag: Rc<Cell<bool>>,
    /// Position shown while the seek slider is being dragged.
    scrub: Option<f64>,
    resume_after_scrub: bool,
    level: f32,
    /// The frame handed to gpui in the last render (and so maybe uploaded to the GPU).
    drawn: Option<ImageId>,
    pub maximized: bool,
}

impl EventEmitter<PlayerEvent> for PlayerView {}

impl PlayerView {
    pub fn new(
        source: Source,
        mixer: Option<Arc<Mixer>>,
        settings: &Settings,
        graveyard: Graveyard,
    ) -> Self {
        let title = source.name().into();
        let player = Player::open(
            source,
            mixer,
            PlayerOptions {
                autoplay: settings.autoplay,
                looping: settings.loop_by_default,
                volume: settings.default_volume,
                max_short_side: settings.max_resolution.short_side(),
                hardware_decoding: settings.hardware_decoding,
            },
        );
        Self {
            player,
            title,
            graveyard,
            seek_drag: Rc::default(),
            volume_drag: Rc::default(),
            scrub: None,
            resume_after_scrub: false,
            level: 0.0,
            drawn: None,
            maximized: false,
        }
    }

    pub fn player(&self) -> &Player {
        &self.player
    }

    pub fn player_mut(&mut self) -> &mut Player {
        &mut self.player
    }

    /// Release a frame that is no longer current. Only frames that were rendered
    /// can have a GPU texture; the rest are simply dropped.
    fn retire(&mut self, image: Arc<RenderImage>) {
        if self.drawn == Some(image.id) {
            self.graveyard.bury(image);
        }
    }

    /// Keeps playback moving (frame queues, end of file, looping) while the
    /// window isn't drawing, for example when it is minimized.
    pub fn background_tick(&mut self, cx: &mut Context<Self>) {
        let was_playing = self.player.is_playing();
        let tick = self.player.tick();
        let advanced = tick.retired.is_some();
        if let Some(image) = tick.retired {
            self.retire(image);
        }
        if advanced || was_playing != self.player.is_playing() {
            cx.notify();
        }
    }

    /// Hand every texture this tile drew to the graveyard; call before dropping it.
    pub fn release_images(&mut self) {
        for image in self.player.take_images() {
            self.graveyard.bury(image);
        }
    }

    fn on_surface_click(&mut self, event: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        // Single click toggles playback. A double click toggles twice, which
        // leaves playback as it was, and maximizes the tile.
        self.player.toggle();
        if event.click_count() == 2 {
            cx.emit(PlayerEvent::ToggleMaximize);
        }
        cx.notify();
    }

    fn on_seek(&mut self, value: f32, slide: Slide, _: &mut Window, cx: &mut Context<Self>) {
        let Some(duration) = self.player.duration() else {
            return;
        };
        let time = value as f64 * duration;
        match slide {
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

    fn on_volume(&mut self, value: f32, _: Slide, _: &mut Window, cx: &mut Context<Self>) {
        self.player.set_volume(value);
        if value > 0.0 && self.player.is_muted() {
            self.player.set_muted(false);
        }
        cx.notify();
    }

    fn render_content(&self, theme: &Theme, fit: ObjectFit) -> AnyElement {
        if let Some(error) = self.player.error() {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .gap_1()
                .max_w(px(420.))
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
            return img(frame).size_full().object_fit(fit).into_any_element();
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
                .size(px(160.))
                .object_fit(ObjectFit::Contain)
                .rounded(px(8.))
                .into_any_element(),
            None => div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(96.))
                .rounded_full()
                .bg(theme.surface_hover)
                .child(icon("music").size(px(40.)).text_color(theme.accent))
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
            .child(
                div()
                    .w(px(160.))
                    .h(px(4.))
                    .rounded_full()
                    .bg(theme.track)
                    .child(
                        div()
                            .h_full()
                            .w(relative(self.level.clamp(0.0, 1.0)))
                            .rounded_full()
                            .bg(theme.accent),
                    ),
            )
            .into_any_element()
    }
}

impl Render for PlayerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tick = self.player.tick();
        if let Some(image) = tick.retired {
            self.retire(image);
        }
        self.drawn = self.player.frame().map(|frame| frame.id);
        let level = self.player.level();
        self.level = if level > self.level {
            level
        } else {
            self.level * 0.88 + level * 0.12
        };
        if tick.animate || self.level > 0.005 {
            window.request_animation_frame();
        }

        let theme = Theme::get(window, cx);
        let settings = cx.global::<Settings>();
        let fit = match settings.video_fit {
            VideoFit::Contain => ObjectFit::Contain,
            VideoFit::Cover => ObjectFit::Cover,
        };
        let show_titles = settings.show_titles;
        let info = self.player.info();
        let has_video = self.player.frame().is_some();
        let dragging = self.seek_drag.get() || self.volume_drag.get();
        let pinned = settings.controls == ControlsVisibility::Always
            || dragging
            || !self.player.is_playing()
            || self.player.error().is_some();

        let group: SharedString = format!("tile-{}", cx.entity_id()).into();
        let view = cx.weak_entity();
        let duration = self.player.duration();
        let live = info.as_ref().is_some_and(|i| i.live);
        // A stream that has started and then stalls; opening has its own message.
        let stalled = self.player.is_buffering()
            && self.player.is_network()
            && (has_video || info.as_ref().is_some_and(|i| !i.has_video));
        let position = self.scrub.unwrap_or_else(|| self.player.position());
        let progress = duration
            .filter(|d| *d > 0.0)
            .map_or(0.0, |d| (position / d) as f32);
        let time_label = match duration {
            Some(d) => format!("{} / {}", format_time(position), format_time(d)),
            None => format_time(position),
        };
        let details = info.as_ref().map(|info| {
            let mut parts = Vec::new();
            if let Some((w, h)) = info.video_size {
                parts.push(format!("{w}×{h}"));
            }
            if let Some(fps) = info.frame_rate {
                parts.push(format!("{} fps", (fps * 100.0).round() / 100.0));
            }
            parts.extend(info.video_codec.as_ref().map(|codec| match info.hardware {
                Some(api) => format!("{codec} ({api})"),
                None => codec.clone(),
            }));
            parts.extend(info.audio_codec.clone());
            parts.join(" · ")
        });

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
        let has_audio = info.as_ref().is_some_and(|i| i.has_audio);
        let looping = self.player.is_looping();
        let reveal = |el: gpui::Div| {
            if pinned {
                el
            } else {
                el.opacity(0.)
                    .group_hover(group.clone(), |style| style.opacity(1.))
            }
        };

        let header = reveal(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .flex()
                .items_center()
                .gap_1()
                .pl_3()
                .pr_1()
                .pt_1()
                .pb_4()
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(overlay::scrim(), 0.),
                    linear_color_stop(overlay::scrim_clear(), 1.),
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_baseline()
                .gap_2()
                .when(show_titles, |this| {
                    this.child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(overlay::text())
                            .child(self.title.clone()),
                    )
                })
                .when_some(details.filter(|d| !d.is_empty()), |this, details| {
                    this.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(overlay::text_muted())
                            .child(details),
                    )
                }),
        )
        .child(
            icon_button(
                "maximize",
                if self.maximized {
                    "minimize"
                } else {
                    "maximize"
                },
                overlay::text(),
                overlay::hover(),
            )
            .on_click(cx.listener(|_, _, _, cx| cx.emit(PlayerEvent::ToggleMaximize))),
        )
        .child(
            icon_button("close", "close", overlay::text(), overlay::hover())
                .on_click(cx.listener(|_, _, _, cx| cx.emit(PlayerEvent::Close))),
        );

        let controls = reveal(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .px_2()
                .pt_6()
                .pb_1()
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(overlay::scrim_clear(), 0.),
                    linear_color_stop(overlay::scrim(), 1.),
                )),
        )
        .when(!live, |this| {
            this.child(div().px_1().child(slider(
                "seek",
                progress,
                self.seek_drag.clone(),
                view.clone(),
                SliderColors {
                    track: overlay::track(),
                    fill: theme.accent,
                    knob: overlay::text(),
                },
                Self::on_seek,
            )))
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_0p5()
                .child(
                    icon_button("play", play_icon, overlay::text(), overlay::hover()).on_click(
                        cx.listener(|this, _, _, cx| {
                            this.player.toggle();
                            cx.notify();
                        }),
                    ),
                )
                .when(live, |this| {
                    this.child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap_1()
                            .ml_1()
                            .px_1p5()
                            .rounded(px(4.))
                            .bg(theme.danger)
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
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
                .when(!live, |this| {
                    this.child(
                        icon_button(
                            "loop",
                            "loop",
                            if looping {
                                theme.accent
                            } else {
                                overlay::text_muted()
                            },
                            overlay::hover(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            let looping = !this.player.is_looping();
                            this.player.set_looping(looping);
                            cx.notify();
                        })),
                    )
                })
                .when(has_audio, |this| {
                    this.child(
                        icon_button("mute", volume_icon, overlay::text(), overlay::hover())
                            .on_click(cx.listener(|this, _, _, cx| {
                                let muted = !this.player.is_muted();
                                this.player.set_muted(muted);
                                cx.notify();
                            })),
                    )
                    .child(div().w(px(72.)).pr_1().child(slider(
                        "volume",
                        if self.player.is_muted() {
                            0.0
                        } else {
                            self.player.volume()
                        },
                        self.volume_drag.clone(),
                        view.clone(),
                        SliderColors {
                            track: overlay::track(),
                            fill: overlay::text(),
                            knob: overlay::text(),
                        },
                        Self::on_volume,
                    )))
                }),
        );

        div()
            .id("player")
            .group(group.clone())
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(8.))
            .bg(if has_video || info.as_ref().is_some_and(|i| i.has_video) {
                black()
            } else {
                theme.tile
            })
            .child(
                div()
                    .id("surface")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(Self::on_surface_click))
                    .child(self.render_content(&theme, fit)),
            )
            .when(stalled, |this| {
                this.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .px_3()
                                .py_1()
                                .rounded_full()
                                .bg(overlay::scrim())
                                .text_xs()
                                .text_color(overlay::text())
                                .child("Buffering…"),
                        ),
                )
            })
            // Clicks on the bars mustn't reach the surface underneath (which
            // toggles playback), but they can't `occlude()` either: an occluding
            // child stops its parent counting as hovered, which would hide the
            // bars as soon as the pointer moved onto them.
            .child(header.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
            .child(controls.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
    }
}
