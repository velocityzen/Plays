//! A window: the toolbar, and a grid with one tile per open file, or the
//! empty window's ring. Each part is in its own file.

mod commands;
mod display;
mod empty;
mod fit;
mod grid;
mod mouse;
mod on_top;
mod opening;
mod place;
#[cfg(test)]
mod tests;
mod tiles;
mod toolbar;

use std::time::{Duration, Instant};

use anyhow::Result;
use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, App, Bounds, Context, DisplayId, Entity, EntityId, ExternalPaths, FocusHandle,
    Pixels, Size, Subscription, TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions,
    black, div, px,
};

use super::displays;
use super::open_url::{OpenUrlDialog, OpenUrlEvent, UrlForm};
use super::player::{Graveyard, PlayerView};
use super::theme::layout::{DROP_INSET, TOOLBAR_HEIGHT, TRAFFIC_LIGHTS, WINDOW_MIN_SIZE};
use super::theme::{self, Theme};
use crate::app::AudioEngine;
use crate::cast::Casting;
use crate::media::picture;
use crate::settings::Settings;

pub use on_top::float_with_players;
pub use place::save_place_on_quit;

/// macOS has the menu bar for what the toolbar does, so its windows show
/// just the video, with the traffic lights over it like other video players.
/// Linux has no menu bar, and the toolbar is what moves the window there
/// when it draws its own decorations.
const TOOLBAR: bool = cfg!(not(target_os = "macos"));
/// The height the toolbar takes over the tiles. The line under it is
/// inside its height, as gpui sizes boxes with their borders.
pub const TOOLBAR_SPACE: Pixels = if TOOLBAR { TOOLBAR_HEIGHT } else { px(0.) };
/// The area under the toolbar, which files are dropped on.
const CONTENT_GROUP: &str = "content";

struct Tile {
    view: Entity<PlayerView>,
    _subscription: Subscription,
}

/// A window: a toolbar and a grid with one tile per open file. Every window
/// is one of these; a tile can move to a new one and keep playing.
pub struct Workspace {
    focus_handle: FocusHandle,
    /// Its own window, to size for the videos opened in it.
    window: AnyWindowHandle,
    /// Files just opened, whose sizes the window waits for.
    fitting: Option<fit::Fitting>,
    tiles: Vec<Tile>,
    /// Gamma for SDR and HDR video, as last applied to the tiles.
    gamma: [f32; 2],
    maximized: Option<EntityId>,
    /// The tile the mouse was last over, or clicked, which shortcuts go to.
    active: Option<EntityId>,
    graveyard: Graveyard,
    open_url: Option<(Entity<OpenUrlDialog>, Subscription)>,
    /// The "Open URL" form, which takes the place of the empty window's
    /// buttons while `url_form_open`.
    url_form: Entity<UrlForm>,
    url_form_open: bool,
    /// The mouse is over the window, so the controls and traffic lights show,
    /// unless it has been resting there (`mouse_idle`).
    mouse_inside: bool,
    mouse_idle: bool,
    mouse_moved_at: Instant,
    /// A timer is waiting to see whether the mouse rests.
    watching_idle: bool,
    traffic_lights_shown: bool,
    /// Floating above other apps' windows, as last set.
    on_top: bool,
    /// The display it is moving to, full screen, once out of full screen
    /// where it is.
    moving_to: Option<u32>,
    /// Where it is, out of full screen.
    place: Option<place::Place>,
    /// Where it was before Output Device put it full screen elsewhere.
    home: Option<place::Home>,
    _settings_subscription: Subscription,
    /// Tiles show a cast button while there are devices to cast to.
    _casting_subscription: Subscription,
    _activation_subscription: Subscription,
    _bounds_subscription: Subscription,
    _url_form_events: Subscription,
}

impl Workspace {
    /// Open an empty window of the given size, centered on the display in
    /// use. On X11, where one display spans every monitor, the window
    /// manager places it.
    pub fn open_window(window_size: Size<Pixels>, cx: &mut App) -> Result<WindowHandle<Self>> {
        let display = displays::in_use(cx);
        let bounds = Bounds::centered(display, window_size, cx);
        Self::open(display, bounds, false, cx)
    }

    /// Open an empty window at `bounds` on `display`, relative to it (the
    /// main display when `None`), and keep it there, X11 too.
    pub(crate) fn open_window_at(
        display: Option<DisplayId>,
        bounds: Bounds<Pixels>,
        cx: &mut App,
    ) -> Result<WindowHandle<Self>> {
        Self::open(display, bounds, true, cx)
    }

    fn open(
        display: Option<DisplayId>,
        bounds: Bounds<Pixels>,
        keep_position: bool,
        cx: &mut App,
    ) -> Result<WindowHandle<Self>> {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                display_id: display,
                keep_position,
                titlebar: Some(TitlebarOptions {
                    title: Some("Plays".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(TRAFFIC_LIGHTS),
                }),
                window_min_size: Some(WINDOW_MIN_SIZE),
                app_id: Some("plays".into()),
                focus: true,
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Self::new(window, cx)),
        )
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        window.set_fullscreen_style(cx.global::<Settings>().full_screen.style());
        let settings_subscription = cx.observe_global_in::<Settings>(window, |this, window, cx| {
            let settings = cx.global::<Settings>();
            window.set_fullscreen_style(settings.full_screen.style());
            let (volume, gamma) = (settings.master_volume, settings.gamma());
            if let Some(mixer) = AudioEngine::mixer(cx) {
                mixer.set_master_volume(volume);
            }
            if gamma != this.gamma {
                this.gamma = gamma;
                picture::set_gamma(gamma[0], gamma[1]);
                for tile in &this.tiles {
                    tile.view
                        .update(cx, |view, _| view.picture_settings_changed());
                }
            }
            cx.notify();
        });
        let casting_subscription = cx.observe_global::<Casting>(|_, cx| cx.notify());
        // Picks up an accent color changed in System Settings meanwhile.
        // The one in front is where the next launch opens.
        let activation_subscription = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                theme::refresh_system_colors(cx);
                this.note_place(window, cx);
            }
        });
        let bounds_subscription =
            cx.observe_window_bounds(window, |this, window, cx| this.note_place(window, cx));
        cx.on_release(|this, cx| this.closed(cx)).detach();
        let url_form = cx.new(|cx| UrlForm::new(cx).centered());
        let url_form_events =
            cx.subscribe_in(&url_form, window, |this, form, event, window, cx| {
                if let OpenUrlEvent::Open(source) = event {
                    this.open_sources(vec![source.clone()], cx);
                }
                form.update(cx, |form, cx| form.clear(cx));
                this.url_form_open = false;
                window.focus(&this.focus_handle);
                cx.notify();
            });
        // Rendering stops while the window is hidden; keep players moving
        // anyway, and the window floating while they play.
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let alive = this.update_in(cx, |this, window, cx| {
                    for tile in &this.tiles {
                        tile.view.update(cx, |view, cx| view.background_tick(cx));
                    }
                    this.update_on_top(window, cx);
                    this.finish_moving(window, cx);
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            focus_handle,
            window: window.window_handle(),
            fitting: None,
            tiles: Vec::new(),
            maximized: None,
            active: None,
            graveyard: Graveyard::default(),
            open_url: None,
            url_form,
            url_form_open: false,
            mouse_inside: true,
            mouse_idle: false,
            mouse_moved_at: Instant::now(),
            watching_idle: false,
            traffic_lights_shown: true,
            on_top: false,
            moving_to: None,
            place: place::Place::of(window, cx),
            home: None,
            gamma: cx.global::<Settings>().gamma(),
            _settings_subscription: settings_subscription,
            _casting_subscription: casting_subscription,
            _activation_subscription: activation_subscription,
            _bounds_subscription: bounds_subscription,
            _url_form_events: url_form_events,
        }
    }
}

/// What smoke runs check.
#[cfg(feature = "smoke")]
impl Workspace {
    /// The tiles' players, in order.
    pub fn players(&self) -> Vec<Entity<PlayerView>> {
        self.tiles.iter().map(|tile| tile.view.clone()).collect()
    }

    /// Whether the window waits for its files' sizes, to fit them.
    pub fn is_fitting(&self) -> bool {
        self.fitting.is_some()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.graveyard.collect(window) {
            window.request_animation_frame();
        }
        self.update_traffic_lights(window, cx);
        self.update_on_top(window, cx);
        self.finish_moving(window, cx);
        let theme = Theme::get(window, cx);
        let drop_highlight = theme.drop_highlight;
        let empty = self.tiles.is_empty();
        // Something was opened another way while the empty window's form was
        // up; it goes away with the ring, and so does its focus.
        if !empty && self.url_form_open {
            self.url_form_open = false;
            window.focus(&self.focus_handle);
        }

        // The dialog sits next to the workspace rather than inside it, so the
        // workspace's single-key shortcuts (space, j, k, l, m) don't fire while
        // typing a URL.
        let workspace = div()
            .id("workspace")
            // Its shortcuts are for players, and single keys such as space and
            // j would otherwise go to them while typing in the ring's field.
            .when(!empty, |this| this.key_context("Workspace"))
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .when(TOOLBAR, |this| {
                this.child(self.render_toolbar(window, cx, &theme))
            })
            .child(
                div()
                    .id("content")
                    .group(CONTENT_GROUP)
                    .flex_1()
                    .min_h_0()
                    // Tiles cover the whole window, so a drag over them shows a
                    // frame around them; an empty window lights up as a whole.
                    .drag_over::<ExternalPaths>(move |style, _, _, _| {
                        let style = style.bg(drop_highlight);
                        if empty { style } else { style.p(DROP_INSET) }
                    })
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        this.open_paths(paths.paths().to_vec(), cx);
                    }))
                    .map(|this| {
                        if empty {
                            this.child(self.render_empty(&theme, window, cx))
                        } else {
                            this.child(self.render_grid(cx))
                        }
                    }),
            );

        div()
            .relative()
            .size_full()
            .bg(if empty { theme.background } else { black() })
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .child(mouse::track_mouse(cx.entity().downgrade()))
            .child(workspace)
            .when_some(self.open_url.as_ref(), |this, (dialog, _)| {
                this.child(dialog.clone())
            })
    }
}
