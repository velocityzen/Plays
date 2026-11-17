//! A window with one player in it, popped out of the main window's grid.

use std::time::Duration;

use anyhow::Result;
use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Entity, FocusHandle, KeyBinding, Pixels, Size, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, actions, black, div, px,
    size,
};

use super::player_view::{Graveyard, PlayerEvent, PlayerView};
use super::theme::Theme;
use crate::media::Player;
use crate::settings::Settings;

actions!(
    player_window,
    [
        PlayPause,
        Forward,
        Backward,
        Restart,
        ToggleMute,
        ToggleFullscreen,
        ExitFullscreen,
    ]
);

const CONTEXT: &str = "PlayerWindow";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", PlayPause, Some(CONTEXT)),
        KeyBinding::new("k", PlayPause, Some(CONTEXT)),
        KeyBinding::new("right", Forward, Some(CONTEXT)),
        KeyBinding::new("l", Forward, Some(CONTEXT)),
        KeyBinding::new("left", Backward, Some(CONTEXT)),
        KeyBinding::new("j", Backward, Some(CONTEXT)),
        KeyBinding::new("home", Restart, Some(CONTEXT)),
        KeyBinding::new("m", ToggleMute, Some(CONTEXT)),
        KeyBinding::new("f", ToggleFullscreen, Some(CONTEXT)),
        KeyBinding::new("escape", ExitFullscreen, Some(CONTEXT)),
    ]);
}

/// Open a window for a player taken out of the grid. It keeps playing.
pub fn open(view: Entity<PlayerView>, cx: &mut App) -> Result<WindowHandle<PlayerWindow>> {
    let (title, size) = {
        let view = view.read(cx);
        (view.title(), window_size(view.player()))
    };
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size, cx))),
            titlebar: Some(TitlebarOptions {
                title: Some(title),
                ..Default::default()
            }),
            window_min_size: Some(MIN_SIZE),
            app_id: Some("plays".into()),
            focus: true,
            ..Default::default()
        },
        |window, cx| cx.new(|cx| PlayerWindow::new(view, window, cx)),
    )
}

const MIN_SIZE: Size<Pixels> = Size {
    width: px(320.),
    height: px(200.),
};

/// The video's own size, scaled down to fit a typical screen; a square for
/// audio, and 16:9 while the file is still opening.
fn window_size(player: &Player) -> Size<Pixels> {
    let info = player.info();
    match info.as_ref().and_then(|i| i.video_size) {
        Some((width, height)) if width > 0 && height > 0 => {
            let (width, height) = (width as f32, height as f32);
            let scale = (1280. / width).min(800. / height).min(1.);
            size(
                px(width * scale).max(MIN_SIZE.width),
                px(height * scale).max(MIN_SIZE.height),
            )
        }
        _ if info.as_ref().is_some_and(|i| !i.has_video) => size(px(420.), px(420.)),
        _ => size(px(960.), px(540.)),
    }
}

pub struct PlayerWindow {
    view: Entity<PlayerView>,
    graveyard: Graveyard,
    focus_handle: FocusHandle,
    _subscription: Subscription,
}

impl PlayerWindow {
    fn new(view: Entity<PlayerView>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let graveyard = Graveyard::default();
        view.update(cx, |view, _| view.move_to_window(graveyard.clone()));
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        let subscription = cx.subscribe_in(&view, window, |_, _, event, window, _| match event {
            PlayerEvent::Close => window.remove_window(),
            PlayerEvent::ToggleMaximize => window.toggle_fullscreen(),
            PlayerEvent::PopOut => {}
        });
        // Rendering stops while the window is hidden; keep the player moving anyway.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    this.view.update(cx, |view, cx| view.background_tick(cx));
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            view,
            graveyard,
            focus_handle,
            _subscription: subscription,
        }
    }

    fn update_player(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut Player)) {
        self.view.update(cx, |view, cx| {
            f(view.player_mut());
            cx.notify();
        });
    }

    fn play_pause(&mut self, _: &PlayPause, _: &mut Window, cx: &mut Context<Self>) {
        self.update_player(cx, |p| p.toggle());
    }

    fn forward(&mut self, _: &Forward, _: &mut Window, cx: &mut Context<Self>) {
        let step = cx.global::<Settings>().seek_step_seconds as f64;
        self.update_player(cx, |p| p.seek_by(step));
    }

    fn backward(&mut self, _: &Backward, _: &mut Window, cx: &mut Context<Self>) {
        let step = cx.global::<Settings>().seek_step_seconds as f64;
        self.update_player(cx, |p| p.seek_by(-step));
    }

    fn restart(&mut self, _: &Restart, _: &mut Window, cx: &mut Context<Self>) {
        self.update_player(cx, |p| p.seek(0.0));
    }

    fn toggle_mute(&mut self, _: &ToggleMute, _: &mut Window, cx: &mut Context<Self>) {
        self.update_player(cx, |p| p.set_muted(!p.is_muted()));
    }

    fn toggle_fullscreen(
        &mut self,
        _: &ToggleFullscreen,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.toggle_fullscreen();
    }

    fn exit_fullscreen(&mut self, _: &ExitFullscreen, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_fullscreen() {
            window.toggle_fullscreen();
        } else {
            cx.propagate();
        }
    }
}

impl Render for PlayerWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.graveyard.collect(window) {
            window.request_animation_frame();
        }
        let theme = Theme::get(window, cx);
        div()
            .id("player-window")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::play_pause))
            .on_action(cx.listener(Self::forward))
            .on_action(cx.listener(Self::backward))
            .on_action(cx.listener(Self::restart))
            .on_action(cx.listener(Self::toggle_mute))
            .on_action(cx.listener(Self::toggle_fullscreen))
            .on_action(cx.listener(Self::exit_fullscreen))
            .size_full()
            .bg(black())
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .child(self.view.clone())
    }
}
