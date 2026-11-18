use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, Decorations, DispatchPhase, Entity, EntityId,
    ExternalPaths, FocusHandle, FontWeight, MouseButton, MouseExitEvent, MouseMoveEvent,
    PathPromptOptions, Pixels, Size, Subscription, TitlebarOptions, WeakEntity, Window,
    WindowBounds, WindowHandle, WindowOptions, black, canvas, div, px, size,
};

use super::components::{Button, IconButton};
use super::open_url::{OpenUrlDialog, OpenUrlEvent, UrlForm};
use super::player_view::{Graveyard, PlayerEvent, PlayerView};
use super::theme::layout::{
    AUDIO_WINDOW_SIZE, COMPACT_WIDTH, DROP_INSET, RING_BORDER, RING_BORDER_DROP,
    RING_FORM_MAX_WIDTH, RING_FORM_MIN_WIDTH, RING_FORM_SCALE, RING_MARGIN, RING_MAX_DIAMETER,
    RING_MIN_DIAMETER, RING_SCALE, TILE_GAP, TOOLBAR_HEIGHT, TRAFFIC_LIGHTS, VIDEO_WINDOW_SIZE,
    WINDOW_MIN_SIZE, WINDOW_SIZE,
};
use super::theme::{self, Theme};
use super::traffic_lights;
use crate::app::{self, AudioEngine};
use crate::media::{Player, Source, picture};
use crate::settings::{ControlsVisibility, Settings};

/// Extensions picked up when a whole folder is opened.
const MEDIA_EXTENSIONS: &[&str] = &[
    "3gp", "aac", "aif", "aiff", "alac", "ape", "avi", "flac", "flv", "gif", "m2ts", "m4a", "m4b",
    "m4v", "mka", "mkv", "mov", "mp2", "mp3", "mp4", "mpeg", "mpg", "mts", "mxf", "oga", "ogg",
    "ogv", "opus", "ts", "vob", "wav", "weba", "webm", "wma", "wmv", "wv",
];

/// macOS has the menu bar for what the toolbar does, so its windows show
/// just the video, with the traffic lights over it like other video players.
/// Linux has no menu bar, and the toolbar is what moves the window there
/// when it draws its own decorations.
const TOOLBAR: bool = cfg!(not(target_os = "macos"));
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
    tiles: Vec<Tile>,
    /// Gamma for SDR and HDR video, as last applied to the tiles.
    gamma: [f32; 2],
    maximized: Option<EntityId>,
    /// The tile last clicked, which shortcuts go to.
    active: Option<EntityId>,
    graveyard: Graveyard,
    open_url: Option<(Entity<OpenUrlDialog>, Subscription)>,
    /// The "Open URL" form, which takes the place of the empty window's
    /// buttons while `url_form_open`.
    url_form: Entity<UrlForm>,
    url_form_open: bool,
    /// The mouse is over the window, so the controls and traffic lights show.
    mouse_inside: bool,
    traffic_lights_shown: bool,
    _settings_subscription: Subscription,
    _activation_subscription: Subscription,
    _url_form_events: Subscription,
}

impl Workspace {
    /// Open an empty window of the given size, centered on the screen.
    pub fn open_window(window_size: Size<Pixels>, cx: &mut App) -> Result<WindowHandle<Self>> {
        let bounds = Bounds::centered(None, window_size, cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
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
        let settings_subscription = cx.observe_global::<Settings>(|this: &mut Self, cx| {
            let settings = cx.global::<Settings>();
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
        // Picks up an accent color changed in System Settings meanwhile.
        let activation_subscription = cx.observe_window_activation(window, |_, window, cx| {
            if window.is_window_active() {
                theme::refresh_system_colors(cx);
            }
        });
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
        // Rendering stops while the window is hidden; keep players moving anyway.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    for tile in &this.tiles {
                        tile.view.update(cx, |view, cx| view.background_tick(cx));
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            focus_handle,
            tiles: Vec::new(),
            maximized: None,
            active: None,
            graveyard: Graveyard::default(),
            open_url: None,
            url_form,
            url_form_open: false,
            mouse_inside: true,
            traffic_lights_shown: true,
            gamma: cx.global::<Settings>().gamma(),
            _settings_subscription: settings_subscription,
            _activation_subscription: activation_subscription,
            _url_form_events: url_form_events,
        }
    }

    pub fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.open_sources(paths.into_iter().map(Source::File).collect(), cx);
    }

    /// Open files, the media files in folders, and streams.
    pub fn open_sources(&mut self, sources: Vec<Source>, cx: &mut Context<Self>) {
        let settings = cx.global::<Settings>().clone();
        let mixer = AudioEngine::mixer(cx);
        let sources = sources.into_iter().flat_map(|source| match source {
            Source::File(path) => expand_folders(vec![path])
                .into_iter()
                .map(Source::File)
                .collect(),
            stream => vec![stream],
        });
        for source in sources {
            log::info!("opening {source}");
            let graveyard = self.graveyard.clone();
            let mixer = mixer.clone();
            let view = cx.new(|_| PlayerView::new(source, mixer, &settings, graveyard));
            self.add_tile(view, cx);
        }
        self.tiles_changed(cx);
    }

    fn add_tile(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        let subscription = cx.subscribe(&view, |this, view, event, cx| match event {
            PlayerEvent::Close => this.close(view.entity_id(), cx),
            PlayerEvent::Activate => {
                this.active = Some(view.entity_id());
                this.tiles_changed(cx);
            }
            PlayerEvent::ToggleMaximize => this.toggle_maximize(view.entity_id(), cx),
            PlayerEvent::PopOut => this.pop_out(view.entity_id(), cx),
        });
        self.tiles.push(Tile {
            view,
            _subscription: subscription,
        });
    }

    /// Tell the tiles where they are and whether the mouse is in the window.
    /// The one in the top-left corner is under the traffic lights.
    fn tiles_changed(&mut self, cx: &mut Context<Self>) {
        let alone = self.tiles.len() == 1;
        let mouse_in_window = self.mouse_inside;
        let top_left = self
            .maximized
            .or_else(|| self.tiles.first().map(|t| t.view.entity_id()));
        let active = self.active_view().map(|view| view.entity_id());
        for tile in &self.tiles {
            let is_top_left = top_left == Some(tile.view.entity_id());
            let is_active = active == Some(tile.view.entity_id());
            tile.view.update(cx, |view, cx| {
                if view.alone != alone
                    || view.top_left != is_top_left
                    || view.mouse_in_window != mouse_in_window
                    || view.active != is_active
                {
                    view.alone = alone;
                    view.top_left = is_top_left;
                    view.mouse_in_window = mouse_in_window;
                    view.active = is_active;
                    cx.notify();
                }
            });
        }
        cx.notify();
    }

    fn set_mouse_inside(&mut self, inside: bool, cx: &mut Context<Self>) {
        if self.mouse_inside != inside {
            self.mouse_inside = inside;
            self.tiles_changed(cx);
        }
    }

    /// Like the controls, the traffic lights fade out while something plays
    /// and the mouse is away from the window.
    fn update_traffic_lights(&mut self, window: &Window, cx: &Context<Self>) {
        let show = self.mouse_inside
            || cx.global::<Settings>().controls == ControlsVisibility::Always
            || !self.any_playing(cx);
        if show != self.traffic_lights_shown {
            self.traffic_lights_shown = show;
            traffic_lights::set_visible(window, show);
        }
    }

    pub fn prompt_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: cx.can_select_mixed_files_and_dirs(),
            multiple: true,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                this.update(cx, |this, cx| this.open_paths(paths, cx)).ok();
            }
        })
        .detach();
    }

    /// Show the "Open URL" form: in place of the empty window's buttons, or
    /// in a sheet over a window that is playing. Focuses it if already up.
    pub fn show_open_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tiles.is_empty() {
            self.url_form_open = true;
            self.url_form.update(cx, |form, cx| form.focus(window, cx));
            cx.notify();
            return;
        }
        if let Some((dialog, _)) = &self.open_url {
            dialog.update(cx, |dialog, cx| dialog.focus(window, cx));
            return;
        }
        let dialog = cx.new(|cx| OpenUrlDialog::new(window, cx));
        let subscription = cx.subscribe_in(&dialog, window, |this, _, event, window, cx| {
            if let OpenUrlEvent::Open(source) = event {
                this.open_sources(vec![source.clone()], cx);
            }
            this.open_url = None;
            window.focus(&this.focus_handle);
            cx.notify();
        });
        self.open_url = Some((dialog, subscription));
        cx.notify();
    }

    fn close(&mut self, id: EntityId, cx: &mut Context<Self>) {
        if let Some(index) = self.tiles.iter().position(|t| t.view.entity_id() == id) {
            let tile = self.tiles.remove(index);
            tile.view.update(cx, |view, _| view.release_pictures());
        }
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        if self.active == Some(id) {
            self.active = None;
        }
        self.tiles_changed(cx);
    }

    /// Move a tile to a new window, sized for it, where it keeps playing.
    fn pop_out(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(index) = self.tiles.iter().position(|t| t.view.entity_id() == id) else {
            return;
        };
        let view = self.tiles[index].view.clone();
        let window_size = window_size_for(view.read(cx).player());
        let window = match Self::open_window(window_size, cx) {
            Ok(window) => window,
            Err(err) => {
                log::error!("could not open a window: {err:#}");
                return;
            }
        };
        self.tiles.remove(index);
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        if self.active == Some(id) {
            self.active = None;
        }
        self.tiles_changed(cx);
        window
            .update(cx, |workspace, _, cx| workspace.adopt(view, cx))
            .ok();
    }

    /// Take in a tile from another window.
    fn adopt(&mut self, view: Entity<PlayerView>, cx: &mut Context<Self>) {
        let graveyard = self.graveyard.clone();
        view.update(cx, |view, _| view.move_to_window(graveyard));
        self.add_tile(view, cx);
        self.tiles_changed(cx);
    }

    pub fn close_all(&mut self, cx: &mut Context<Self>) {
        for tile in self.tiles.drain(..) {
            tile.view.update(cx, |view, _| view.release_pictures());
        }
        self.maximized = None;
        self.active = None;
        self.tiles_changed(cx);
    }

    fn toggle_maximize(&mut self, id: EntityId, cx: &mut Context<Self>) {
        self.maximized = if self.maximized == Some(id) {
            None
        } else {
            Some(id)
        };
        for tile in &self.tiles {
            let maximized = self.maximized == Some(tile.view.entity_id());
            tile.view.update(cx, |view, _| view.maximized = maximized);
        }
        self.tiles_changed(cx);
    }

    /// The tile shortcuts go to: the maximized one, else the one last
    /// clicked, else the first.
    fn active_view(&self) -> Option<Entity<PlayerView>> {
        let find = |id: EntityId| self.tiles.iter().find(|t| t.view.entity_id() == id);
        self.maximized
            .and_then(find)
            .or_else(|| self.active.and_then(find))
            .or_else(|| self.tiles.first())
            .map(|tile| tile.view.clone())
    }

    /// Run `f` on the tile shortcuts go to.
    pub fn with_active(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut PlayerView, &mut Context<PlayerView>),
    ) {
        if let Some(view) = self.active_view() {
            view.update(cx, f);
        }
    }

    fn for_each_player(&mut self, cx: &mut Context<Self>, f: impl Fn(&mut crate::media::Player)) {
        for tile in &self.tiles {
            tile.view.update(cx, |view, cx| {
                f(view.player_mut());
                cx.notify();
            });
        }
    }

    fn any_playing(&self, cx: &Context<Self>) -> bool {
        self.tiles
            .iter()
            .any(|t| t.view.read(cx).player().is_playing())
    }

    pub fn play_all(&mut self, cx: &mut Context<Self>) {
        self.for_each_player(cx, |p| p.play());
    }

    pub fn pause_all(&mut self, cx: &mut Context<Self>) {
        self.for_each_player(cx, |p| p.pause());
    }

    pub fn toggle_all(&mut self, cx: &mut Context<Self>) {
        if self.any_playing(cx) {
            self.pause_all(cx);
        } else {
            self.play_all(cx);
        }
    }

    pub fn restart_all(&mut self, cx: &mut Context<Self>) {
        self.for_each_player(cx, |p| p.seek(0.0));
    }

    pub fn seek_all(&mut self, direction: f64, cx: &mut Context<Self>) {
        let step = cx.global::<Settings>().seek_step_seconds as f64 * direction;
        self.for_each_player(cx, |p| p.seek_by(step));
    }

    pub fn toggle_mute_all(&mut self, cx: &mut Context<Self>) {
        let mute = self
            .tiles
            .iter()
            .any(|t| !t.view.read(cx).player().is_muted());
        self.for_each_player(cx, |p| p.set_muted(mute));
    }

    fn render_toolbar(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        theme: &Theme,
    ) -> impl IntoElement {
        let client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let count = self.tiles.len();
        let playing = self.any_playing(cx);
        let streams = self
            .tiles
            .iter()
            .filter(|t| t.view.read(cx).player().is_network())
            .count();
        let plural = |n: usize, one: &str, many: &str| match n {
            1 => format!("1 {one}"),
            n => format!("{n} {many}"),
        };
        let compact = window.viewport_size().width < COMPACT_WIDTH;
        let status = match (count - streams, streams) {
            (0, 0) => "Nothing open".to_string(),
            (files, 0) => plural(files, "file", "files"),
            (0, streams) => plural(streams, "stream", "streams"),
            (files, streams) => format!(
                "{} · {}",
                plural(files, "file", "files"),
                plural(streams, "stream", "streams")
            ),
        };

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(TOOLBAR_HEIGHT)
            .px_2()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .child(tool(
                compact,
                "open",
                "plus",
                "Open…",
                cx.listener(|this, _, window, cx| this.prompt_open(window, cx)),
            ))
            .child(tool(
                compact,
                "open-url",
                "link",
                "Open URL…",
                cx.listener(|this, _, window, cx| this.show_open_url(window, cx)),
            ))
            .when(count > 0, |this| {
                this.child(tool(
                    compact,
                    "play-pause-all",
                    if playing { "pause" } else { "play" },
                    if playing { "Pause all" } else { "Play all" },
                    cx.listener(|this, _, _, cx| this.toggle_all(cx)),
                ))
                .child(tool(
                    compact,
                    "restart-all",
                    "replay",
                    "Restart all",
                    cx.listener(|this, _, _, cx| this.restart_all(cx)),
                ))
                .child(tool(
                    compact,
                    "close-all",
                    "close",
                    "Close all",
                    cx.listener(|this, _, _, cx| this.close_all(cx)),
                ))
            })
            .child(
                // Empty toolbar space moves the window when we draw our own decorations.
                div()
                    .id("drag-area")
                    .flex_1()
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_end()
                    .px_2()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .on_mouse_down(MouseButton::Left, move |_, window, _| {
                        if client_decorated {
                            window.start_window_move();
                        }
                    })
                    .child(status),
            )
            .child(
                IconButton::new("settings", "settings")
                    .color(theme.text_muted)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(app::OpenSettings), cx)
                    }),
            )
            .when(client_decorated, |this| {
                this.child(
                    IconButton::new("minimize-window", "window-min")
                        .color(theme.text_muted)
                        .on_click(|_, window, _| window.minimize_window()),
                )
                .child(
                    IconButton::new("zoom-window", "window-max")
                        .color(theme.text_muted)
                        .on_click(|_, window, _| window.zoom_window()),
                )
                .child(
                    IconButton::new("close-window", "close")
                        .color(theme.text_muted)
                        .on_click(|_, window, _| window.remove_window()),
                )
            })
    }

    /// A ring like the one in the app's icon, as large as the window allows,
    /// with the open buttons inside.
    fn render_empty(
        &self,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (open_shortcut, url_shortcut) = if cfg!(target_os = "macos") {
            ("⌘O", "⇧⌘O")
        } else {
            ("Ctrl+O", "Shift+Ctrl+O")
        };
        let viewport = window.viewport_size();
        let height = if TOOLBAR {
            viewport.height - TOOLBAR_HEIGHT - px(1.)
        } else {
            viewport.height
        };
        let shorter = viewport.width.min(height);
        // Small windows get a larger share, so what is inside still fits.
        let diameter = (shorter * RING_SCALE)
            .max(RING_MIN_DIAMETER.min(shorter - RING_MARGIN))
            .min(RING_MAX_DIAMETER);
        let form_width = (diameter * RING_FORM_SCALE)
            .max(RING_FORM_MIN_WIDTH)
            .min(RING_FORM_MAX_WIDTH);
        let accent = theme.accent;

        let ring = div()
            .flex_none()
            .size(diameter)
            .rounded_full()
            .border_2()
            .border_color(theme.text.opacity(RING_BORDER))
            .group_drag_over::<ExternalPaths>(CONTENT_GROUP, move |style| {
                style.border_color(accent.opacity(RING_BORDER_DROP))
            })
            .flex()
            .flex_col()
            .justify_center()
            .text_center()
            // The title sits in the very middle, with the rest below it.
            .child(
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .child("It plays for you"),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_full()
                            .left_0()
                            .right_0()
                            .pt_4()
                            .flex()
                            .justify_center()
                            .map(|this| {
                                if self.url_form_open {
                                    this.child(div().w(form_width).child(self.url_form.clone()))
                                } else {
                                    this.gap_2()
                                        .child(
                                            Button::new("open-empty", "Open")
                                                .primary()
                                                .wide()
                                                .shortcut(open_shortcut)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.prompt_open(window, cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("open-url-empty", "Open URL")
                                                .wide()
                                                .shortcut(url_shortcut)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.show_open_url(window, cx)
                                                })),
                                        )
                                }
                            }),
                    ),
            );

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(ring)
    }

    fn render_grid(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let maximized = self
            .maximized
            .and_then(|id| self.tiles.iter().find(|t| t.view.entity_id() == id));
        let visible: Vec<&Tile> = match maximized {
            Some(tile) => vec![tile],
            None => self.tiles.iter().collect(),
        };
        let columns = cx
            .global::<Settings>()
            .columns_for(visible.len())
            .clamp(1, visible.len().max(1));

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(TILE_GAP)
            .children(visible.chunks(columns).map(|row| {
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap(TILE_GAP)
                    .children(
                        row.iter()
                            .map(|tile| div().flex_1().min_w_0().h_full().child(tile.view.clone())),
                    )
                    .children((row.len()..columns).map(|_| div().flex_1().min_w_0()))
            }))
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.graveyard.collect(window) {
            window.request_animation_frame();
        }
        self.update_traffic_lights(window, cx);
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
            .child(track_mouse(cx.entity().downgrade()))
            .child(workspace)
            .when_some(self.open_url.as_ref(), |this, (dialog, _)| {
                this.child(dialog.clone())
            })
    }
}

/// Follows the mouse in and out of the window. gpui keeps the last position
/// after the mouse leaves, so hovering alone would leave things showing.
fn track_mouse(workspace: WeakEntity<Workspace>) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            window.on_mouse_event({
                let workspace = workspace.clone();
                move |_: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        workspace
                            .update(cx, |this, cx| this.set_mouse_inside(true, cx))
                            .ok();
                    }
                }
            });
            window.on_mouse_event(move |_: &MouseExitEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    workspace
                        .update(cx, |this, cx| this.set_mouse_inside(false, cx))
                        .ok();
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// A toolbar button. Narrow windows show just its icon so the toolbar still fits.
fn tool(
    compact: bool,
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    if compact {
        IconButton::new(id, icon)
            .on_click(on_click)
            .into_any_element()
    } else {
        Button::new(id, label)
            .icon(icon)
            .on_click(on_click)
            .into_any_element()
    }
}

/// A window for one file: the video at its own size (scaled down to fit a
/// typical screen), a square for audio, and 16:9 while the file is still
/// opening.
fn window_size_for(player: &Player) -> Size<Pixels> {
    let info = player.info();
    let content = match info.as_ref().and_then(|i| i.video_size) {
        Some((width, height)) if width > 0 && height > 0 => {
            let (width, height) = (width as f32, height as f32);
            let scale = (WINDOW_SIZE.width / px(width))
                .min(WINDOW_SIZE.height / px(height))
                .min(1.);
            size(px(width * scale), px(height * scale))
        }
        _ if info.as_ref().is_some_and(|i| !i.has_video) => AUDIO_WINDOW_SIZE,
        _ => VIDEO_WINDOW_SIZE,
    };
    // The toolbar has a 1px border under it.
    let toolbar = if TOOLBAR {
        TOOLBAR_HEIGHT + px(1.)
    } else {
        px(0.)
    };
    size(
        content.width.max(WINDOW_MIN_SIZE.width),
        (content.height + toolbar).max(WINDOW_MIN_SIZE.height),
    )
}

/// Replace folders with the media files directly inside them.
fn expand_folders(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&path)
                .map(|dir| dir.flatten().map(|e| e.path()).collect())
                .unwrap_or_default();
            entries.sort();
            files.extend(entries.into_iter().filter(|p| p.is_file() && is_media(p)));
        } else {
            files.push(path);
        }
    }
    files
}

fn is_media(path: &Path) -> bool {
    let hidden = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'));
    let known = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MEDIA_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()));
    known && !hidden
}
