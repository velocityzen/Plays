//! The Settings window: a sidebar of panes, and the pane picked. Each pane
//! is in its own file, with the name and icon the sidebar shows for it.

mod about;
mod appearance;
mod audio;
mod form;
mod hotkeys;
mod playback;
mod screenshots;
mod subtitles;
mod video;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, FocusHandle, Global, KeyDownEvent, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowHandle, WindowOptions, div, rgb, white,
};

use super::components::{Badge, Button};
use super::theme::layout::{SETTINGS_WINDOW_MIN_SIZE, SETTINGS_WINDOW_SIZE, TRAFFIC_LIGHTS};
use super::theme::settings::{
    SIDEBAR_ROW_HEIGHT, SIDEBAR_TOP, SIDEBAR_TOP_MAC, SIDEBAR_WIDTH, TITLE_HEIGHT,
};
use super::theme::text::TextStyle;
use super::theme::{Theme, radius};
use crate::settings::Settings;
use form::update_settings;

/// A pane of the Settings window, and its entry in the sidebar.
trait Pane {
    /// Its name, in the sidebar and as the pane's title.
    fn name(&self) -> &'static str;

    /// Its icon, on a badge in the sidebar.
    fn icon(&self) -> &'static str;

    /// The badge's color, from `theme::settings`.
    fn color(&self) -> u32;

    /// Its groups of rows, which scroll.
    fn content(
        &self,
        view: &mut SettingsView,
        settings: &Settings,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement>;

    /// What stays in view above the rows while they scroll.
    fn header(&self, _settings: &Settings, _theme: &Theme) -> Option<AnyElement> {
        None
    }

    /// A key pressed while the pane shows.
    fn key_down(
        &self,
        _view: &mut SettingsView,
        _event: &KeyDownEvent,
        _cx: &mut Context<SettingsView>,
    ) {
    }

    /// Another pane is picked: close what was left open in this one.
    fn leave(&self, _view: &mut SettingsView) {}
}

/// The panes, in the sidebar's order.
const PANES: [&dyn Pane; 8] = [
    &playback::PlaybackPane,
    &audio::AudioPane,
    &video::VideoPane,
    &subtitles::SubtitlesPane,
    &screenshots::ScreenshotsPane,
    &hotkeys::HotkeysPane,
    &appearance::AppearancePane,
    &about::AboutPane,
];

pub struct SettingsView {
    focus_handle: FocusHandle,
    /// The pane showing, in `PANES`.
    pane: usize,
    /// Whether each slider is being dragged, by its id.
    drags: HashMap<&'static str, Rc<Cell<bool>>>,
    font_menu: subtitles::FontMenu,
    /// Whether the menu of audio outputs is open.
    device_menu: bool,
    recording: hotkeys::Recording,
    /// The credit in About whose license shows.
    license: Option<SharedString>,
}

/// The Settings window, once it has been opened.
struct SettingsWindow(WindowHandle<SettingsView>);
impl Global for SettingsWindow {}

impl SettingsView {
    /// Bring the Settings window to the front, opening it if it isn't open.
    pub fn open_window(cx: &mut App) {
        if let Some(handle) = cx.try_global::<SettingsWindow>().map(|open| open.0)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let bounds = Bounds::centered(None, SETTINGS_WINDOW_SIZE, cx);
        // On macOS the sidebar runs up under the traffic lights, as in System
        // Settings.
        let mac = cfg!(target_os = "macos");
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Plays Settings".into()),
                    appears_transparent: mac,
                    traffic_light_position: mac.then_some(TRAFFIC_LIGHTS),
                }),
                window_min_size: Some(SETTINGS_WINDOW_MIN_SIZE),
                app_id: Some("plays".into()),
                focus: true,
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Self::new(window, cx)),
        );
        match opened {
            Ok(handle) => cx.set_global(SettingsWindow(handle)),
            Err(err) => log::error!("could not open settings: {err:#}"),
        }
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        Self {
            focus_handle,
            pane: 0,
            drags: HashMap::new(),
            font_menu: subtitles::FontMenu::default(),
            device_menu: false,
            recording: hotkeys::Recording::default(),
            license: None,
        }
    }

    /// Whether the slider `id` is being dragged, which it keeps between
    /// frames.
    fn drag(&mut self, id: &'static str) -> Rc<Cell<bool>> {
        self.drags.entry(id).or_default().clone()
    }

    fn render_sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex_none()
            .w(SIDEBAR_WIDTH)
            .h_full()
            .flex()
            .flex_col()
            .gap_0p5()
            .px_2p5()
            // Below the traffic lights, which sit over the sidebar on macOS.
            .pt(if cfg!(target_os = "macos") {
                SIDEBAR_TOP_MAC
            } else {
                SIDEBAR_TOP
            })
            .bg(theme.tile)
            .border_r_1()
            .border_color(theme.border)
            .children(PANES.iter().enumerate().map(|(index, pane)| {
                let selected = index == self.pane;
                div()
                    .id(pane.name())
                    .flex()
                    .items_center()
                    .gap_2()
                    .h(SIDEBAR_ROW_HEIGHT)
                    .px_1p5()
                    .superellipse(radius::FRAME)
                    .text_sm()
                    .map(|this| {
                        if selected {
                            this.bg(theme.accent).text_color(white())
                        } else {
                            this.hover(|style| style.bg(theme.surface_hover))
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        PANES[this.pane].leave(this);
                        this.pane = index;
                        cx.notify();
                    }))
                    .child(Badge::icon(pane.icon(), rgb(pane.color())))
                    .child(pane.name())
            }))
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let settings = cx.global::<Settings>().clone();
        let path_hint = Settings::path()
            .map(|p| format!("Saved to {}", p.display()))
            .unwrap_or_else(|| "Settings can't be saved on this system".into());
        let index = self.pane;
        let pane = PANES[index];
        let content = pane.content(self, &settings, &theme, window, cx);

        let body = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .child(
                // In line with the traffic lights on macOS, like a title.
                div()
                    .flex_none()
                    .h(TITLE_HEIGHT)
                    .px_5()
                    .flex()
                    .items_center()
                    .text_title()
                    .child(pane.name()),
            )
            .when_some(pane.header(&settings, &theme), |this, header| {
                this.child(div().flex_none().px_5().pt_2().pb_1().child(header))
            })
            .child(
                div()
                    // Each pane scrolls on its own and opens at the top.
                    .id(("settings-pane", index))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .px_5()
                    .pt_2()
                    .pb_5()
                    .children(content),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_5()
                    .py_3()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child(path_hint),
                    )
                    .child(
                        Button::new("reset", "Reset to defaults").on_click(|_, _, cx| {
                            update_settings(cx, true, |s| *s = Settings::default())
                        }),
                    ),
            );

        div()
            .id("settings")
            .key_context("Settings")
            .track_focus(&self.focus_handle)
            .on_key_down(
                cx.listener(|this, event, _, cx| PANES[this.pane].key_down(this, event, cx)),
            )
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .child(self.render_sidebar(&theme, cx))
            .child(body)
    }
}
