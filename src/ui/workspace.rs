use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    Context, Decorations, Entity, EntityId, ExternalPaths, FocusHandle, MouseButton,
    PathPromptOptions, Subscription, Window, div, px,
};

use super::open_url::{OpenUrlDialog, OpenUrlEvent};
use super::player_view::{Graveyard, PlayerEvent, PlayerView};
use super::theme::Theme;
use super::widgets::{button, icon, icon_button};
use crate::app::{self, AudioEngine};
use crate::media::Source;
use crate::settings::Settings;

/// Extensions picked up when a whole folder is opened.
const MEDIA_EXTENSIONS: &[&str] = &[
    "3gp", "aac", "aif", "aiff", "alac", "ape", "avi", "flac", "flv", "gif", "m2ts", "m4a", "m4b",
    "m4v", "mka", "mkv", "mov", "mp2", "mp3", "mp4", "mpeg", "mpg", "mts", "mxf", "oga", "ogg",
    "ogv", "opus", "ts", "vob", "wav", "weba", "webm", "wma", "wmv", "wv",
];

struct Tile {
    view: Entity<PlayerView>,
    _subscription: Subscription,
}

/// The main window: a toolbar and a grid with one tile per open file.
pub struct Workspace {
    focus_handle: FocusHandle,
    tiles: Vec<Tile>,
    maximized: Option<EntityId>,
    graveyard: Graveyard,
    open_url: Option<(Entity<OpenUrlDialog>, Subscription)>,
    _settings_subscription: Subscription,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        let settings_subscription = cx.observe_global::<Settings>(|_, cx| {
            let volume = cx.global::<Settings>().master_volume;
            if let Some(mixer) = AudioEngine::mixer(cx) {
                mixer.set_master_volume(volume);
            }
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
            graveyard: Graveyard::default(),
            open_url: None,
            _settings_subscription: settings_subscription,
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
            let subscription = cx.subscribe(&view, |this, view, event, cx| match event {
                PlayerEvent::Close => this.close(view.entity_id(), cx),
                PlayerEvent::ToggleMaximize => this.toggle_maximize(view.entity_id(), cx),
            });
            self.tiles.push(Tile {
                view,
                _subscription: subscription,
            });
        }
        cx.notify();
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

    /// Show the "Open URL" dialog, or focus it if it is already up.
    pub fn show_open_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((dialog, _)) = &self.open_url {
            dialog.read(cx).focus(window, cx);
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
            tile.view.update(cx, |view, _| view.release_images());
        }
        if self.maximized == Some(id) {
            self.maximized = None;
        }
        cx.notify();
    }

    pub fn close_all(&mut self, cx: &mut Context<Self>) {
        for tile in self.tiles.drain(..) {
            tile.view.update(cx, |view, _| view.release_images());
        }
        self.maximized = None;
        cx.notify();
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
        cx.notify();
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
        // Narrow windows get icon-only buttons so the toolbar still fits.
        let compact = window.viewport_size().width < px(760.);
        let tool = |id: &'static str, icon_name: &'static str, label: &'static str| {
            if compact {
                icon_button(id, icon_name, theme.text, theme.surface_hover)
            } else {
                button(id, Some(icon_name), label, theme)
            }
        };
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
            .h(px(44.))
            .px_2()
            // Leave room for the macOS traffic lights.
            .when(cfg!(target_os = "macos"), |this| this.pl(px(80.)))
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .child(
                tool("open", "plus", "Open…")
                    .on_click(cx.listener(|this, _, window, cx| this.prompt_open(window, cx))),
            )
            .child(
                tool("open-url", "link", "Open URL…")
                    .on_click(cx.listener(|this, _, window, cx| this.show_open_url(window, cx))),
            )
            .when(count > 0, |this| {
                this.child(
                    tool(
                        "play-pause-all",
                        if playing { "pause" } else { "play" },
                        if playing { "Pause all" } else { "Play all" },
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_all(cx))),
                )
                .child(
                    tool("restart-all", "replay", "Restart all")
                        .on_click(cx.listener(|this, _, _, cx| this.restart_all(cx))),
                )
                .child(
                    tool("close-all", "close", "Close all")
                        .on_click(cx.listener(|this, _, _, cx| this.close_all(cx))),
                )
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
                icon_button(
                    "settings",
                    "settings",
                    theme.text_muted,
                    theme.surface_hover,
                )
                .on_click(|_, window, cx| window.dispatch_action(Box::new(app::OpenSettings), cx)),
            )
            .when(client_decorated, |this| {
                this.child(
                    icon_button(
                        "minimize-window",
                        "window-min",
                        theme.text_muted,
                        theme.surface_hover,
                    )
                    .on_click(|_, window, _| window.minimize_window()),
                )
                .child(
                    icon_button(
                        "zoom-window",
                        "window-max",
                        theme.text_muted,
                        theme.surface_hover,
                    )
                    .on_click(|_, window, _| window.zoom_window()),
                )
                .child(
                    icon_button(
                        "close-window",
                        "close",
                        theme.text_muted,
                        theme.surface_hover,
                    )
                    .on_click(|_, window, _| window.remove_window()),
                )
            })
    }

    fn render_empty(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let (open_shortcut, url_shortcut) = if cfg!(target_os = "macos") {
            ("⌘O", "⇧⌘O")
        } else {
            ("Ctrl+O", "Shift+Ctrl+O")
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .rounded(px(12.))
            .border_2()
            .border_color(theme.border)
            .child(icon("music").size(px(36.)).text_color(theme.text_muted))
            .child(div().text_lg().child("Drop videos and audio files here"))
            .child(div().text_sm().text_color(theme.text_muted).child(format!(
                "Everything you open plays side by side. Press {open_shortcut} to browse, \
                 or {url_shortcut} to open a stream."
            )))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        button("open-empty", Some("plus"), "Open files…", theme).on_click(
                            cx.listener(|this, _, window, cx| this.prompt_open(window, cx)),
                        ),
                    )
                    .child(
                        button("open-url-empty", Some("link"), "Open URL…", theme).on_click(
                            cx.listener(|this, _, window, cx| this.show_open_url(window, cx)),
                        ),
                    ),
            )
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
            .gap_1p5()
            .children(visible.chunks(columns).map(|row| {
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .gap_1p5()
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
        let theme = Theme::get(window, cx);
        let drop_highlight = theme.drop_highlight;

        // The dialog sits next to the workspace rather than inside it, so the
        // workspace's single-key shortcuts (space, j, k, l, m) don't fire while
        // typing a URL.
        let workspace = div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_toolbar(window, cx, &theme))
            .child(
                div()
                    .id("content")
                    .flex_1()
                    .min_h_0()
                    .p_1p5()
                    .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(drop_highlight))
                    .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        this.open_paths(paths.paths().to_vec(), cx);
                    }))
                    .map(|this| {
                        if self.tiles.is_empty() {
                            this.child(self.render_empty(&theme, cx))
                        } else {
                            this.child(self.render_grid(cx))
                        }
                    }),
            );

        div()
            .relative()
            .size_full()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .child(workspace)
            .when_some(self.open_url.as_ref(), |this, (dialog, _)| {
                this.child(dialog.clone())
            })
    }
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
