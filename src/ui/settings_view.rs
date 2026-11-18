use std::cell::Cell;
use std::ops::{Range, RangeInclusive};
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnchoredPositionMode, AnyElement, App, Bounds, ClickEvent, Context, Corner, FocusHandle,
    FontWeight, Global, Hsla, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, Pixels,
    ScrollStrategy, SharedString, TitlebarOptions, UniformListScrollHandle, Window, WindowBounds,
    WindowHandle, WindowOptions, anchored, black, canvas, deferred, div, fill, linear_color_stop,
    linear_gradient, point, px, rgb, size, transparent_black, uniform_list, white,
};

use super::components::{Button, Icon, Panel, Segmented, Slide, Slider, Switch};
use super::subtitles;
use super::theme::layout::{SETTINGS_WINDOW_MIN_SIZE, SETTINGS_WINDOW_SIZE, TRAFFIC_LIGHTS};
use super::theme::settings::{
    APPEARANCE_COLOR, AUDIO_COLOR, BADGE, BADGE_ICON, FONT_BUTTON_WIDTH, FONT_CHECK_WIDTH,
    FONT_MENU_HEIGHT, FONT_MENU_TOP, FONT_MENU_WIDTH, HOTKEY_ADD_ICON, HOTKEY_CHIP_HEIGHT,
    HOTKEY_KEYS_MAX_WIDTH, HOTKEY_REMOVE_ICON, HOTKEYS_COLOR, OUTPUT_MAX_WIDTH, PLAYBACK_COLOR,
    PREVIEW_PICTURE, PREVIEW_SIZE, PREVIEW_SKY, PREVIEW_SUN, PREVIEW_SUN_CENTER, PREVIEW_SUN_SIZE,
    SIDEBAR_ROW_HEIGHT, SIDEBAR_TOP, SIDEBAR_TOP_MAC, SIDEBAR_WIDTH, SLIDER_VALUE_WIDTH,
    SLIDER_WIDTH, SUBTITLES_COLOR, SWATCH, SWATCH_PADDING, TITLE_HEIGHT, VIDEO_COLOR,
};
use super::theme::{Theme, controls, menu, radius};
use crate::app::AudioEngine;
use crate::hotkeys::{self, Command, Group, Hotkeys};
use crate::media::subtitles::{CueContent, parse_ass};
use crate::settings::{
    ControlsVisibility, GAMMA_RANGE, MaxResolution, Rgb, SUBTITLE_COLORS, SUBTITLE_POSITION_RANGE,
    SUBTITLE_SIZE_RANGE, Settings, SubtitleArea, SubtitleEdge, SubtitleStyle, SubtitleWeight,
    ThemeMode, VideoFit,
};

fn update_settings(cx: &mut App, save: bool, change: impl FnOnce(&mut Settings)) {
    Settings::update(cx, save, change);
}

/// The settings window's sections: a sidebar entry and a pane each.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Playback,
    Audio,
    Video,
    Subtitles,
    Hotkeys,
    Appearance,
}

impl Section {
    const ALL: [Self; 6] = [
        Self::Playback,
        Self::Audio,
        Self::Video,
        Self::Subtitles,
        Self::Hotkeys,
        Self::Appearance,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Playback => "Playback",
            Self::Audio => "Audio",
            Self::Video => "Video",
            Self::Subtitles => "Subtitles",
            Self::Hotkeys => "Hotkeys",
            Self::Appearance => "Appearance",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Playback => "play",
            Self::Audio => "volume",
            Self::Video => "display",
            Self::Subtitles => "subtitles",
            Self::Hotkeys => "keyboard",
            Self::Appearance => "appearance",
        }
    }

    /// The badge behind the icon, in macOS's system colors, as System
    /// Settings has them.
    fn color(self) -> Hsla {
        rgb(match self {
            Self::Playback => PLAYBACK_COLOR,
            Self::Audio => AUDIO_COLOR,
            Self::Video => VIDEO_COLOR,
            Self::Subtitles => SUBTITLES_COLOR,
            Self::Hotkeys => HOTKEYS_COLOR,
            Self::Appearance => APPEARANCE_COLOR,
        })
        .into()
    }
}

pub struct SettingsView {
    focus_handle: FocusHandle,
    section: Section,
    master_drag: Rc<Cell<bool>>,
    default_drag: Rc<Cell<bool>>,
    sdr_gamma_drag: Rc<Cell<bool>>,
    hdr_gamma_drag: Rc<Cell<bool>>,
    subtitle_size_drag: Rc<Cell<bool>>,
    subtitle_background_drag: Rc<Cell<bool>>,
    subtitle_position_drag: Rc<Cell<bool>>,
    /// The subtitle font menu is open.
    fonts_open: bool,
    /// The installed font families, listed the first time the menu opens.
    fonts: Rc<[SharedString]>,
    font_scroll: UniformListScrollHandle,
    /// The command whose next keystroke becomes one of its shortcuts.
    recording: Option<Command>,
    /// What the last shortcut recorded did to another command.
    hotkey_note: Option<(Command, SharedString)>,
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
            section: Section::Playback,
            master_drag: Rc::default(),
            default_drag: Rc::default(),
            sdr_gamma_drag: Rc::default(),
            hdr_gamma_drag: Rc::default(),
            subtitle_size_drag: Rc::default(),
            subtitle_background_drag: Rc::default(),
            subtitle_position_drag: Rc::default(),
            fonts_open: false,
            fonts: Rc::new([]),
            font_scroll: UniformListScrollHandle::new(),
            recording: None,
            hotkey_note: None,
        }
    }
}

/// Rows on one card, under an optional heading.
fn group(title: Option<&'static str>, theme: &Theme, rows: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .when_some(title, |this, title| {
            this.child(
                div()
                    .px_1()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .superellipse(radius::CARD)
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .children(rows.into_iter().enumerate().map(|(index, row)| {
                    div()
                        .when(index > 0, |this| {
                            this.border_t_1().border_color(theme.border)
                        })
                        .child(row)
                })),
        )
        .into_any_element()
}

fn row(
    label: &'static str,
    hint: Option<SharedString>,
    theme: &Theme,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_3()
        .py_2p5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .child(div().text_sm().child(label))
                .when_some(hint, |this, hint| {
                    this.child(div().text_xs().text_color(theme.text_muted).child(hint))
                }),
        )
        .child(control)
        .into_any_element()
}

fn percent_slider(
    id: &'static str,
    value: f32,
    dragging: Rc<Cell<bool>>,
    theme: &Theme,
    apply: impl Fn(&mut Settings, f32) + Copy + 'static,
) -> impl IntoElement {
    range_slider(id, value, 0.0..=1.0, 100.0, dragging, theme, apply)
}

/// A slider over `range`, shown as a percentage, that moves in steps of
/// `1 / steps` so it lands on round numbers.
fn range_slider(
    id: &'static str,
    value: f32,
    range: RangeInclusive<f32>,
    steps: f32,
    dragging: Rc<Cell<bool>>,
    theme: &Theme,
    apply: impl Fn(&mut Settings, f32) + Copy + 'static,
) -> impl IntoElement {
    let (low, high) = (*range.start(), *range.end());
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .w(SLIDER_WIDTH)
        .child(
            Slider::new(id, (value - low) / (high - low), dragging).on_change(
                move |event, _, cx| {
                    let value = ((low + event.value * (high - low)) * steps).round() / steps;
                    update_settings(cx, event.slide == Slide::End, |s| apply(s, value));
                },
            ),
        )
        .child(
            div()
                .w(SLIDER_VALUE_WIDTH)
                .flex_none()
                .text_right()
                .text_sm()
                .text_color(theme.text_muted)
                .child(format!("{}%", (value * 100.0).round())),
        )
}

/// A gamma slider. Gamma is spread evenly by its logarithm, so 1 sits in the
/// middle, and moves in steps of 0.05, so it comes back to exactly 1.
fn gamma_slider(
    id: &'static str,
    value: f32,
    dragging: Rc<Cell<bool>>,
    theme: &Theme,
    apply: impl Fn(&mut Settings, f32) + Copy + 'static,
) -> impl IntoElement {
    let (low, high) = (GAMMA_RANGE.start().log2(), GAMMA_RANGE.end().log2());
    let position = (value.log2() - low) / (high - low);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .w(SLIDER_WIDTH)
        .child(
            Slider::new(id, position, dragging).on_change(move |event, _, cx| {
                let gamma = (low + event.value * (high - low)).exp2();
                let gamma = (gamma * 20.0).round() / 20.0;
                update_settings(cx, event.slide == Slide::End, |s| apply(s, gamma));
            }),
        )
        .child(
            div()
                .w(SLIDER_VALUE_WIDTH)
                .flex_none()
                .text_right()
                .text_sm()
                .text_color(theme.text_muted)
                .child(format!("{value:.2}")),
        )
}

/// A row of colors to pick one from, for the subtitle style's color that
/// `apply` sets.
fn swatches(
    id: &'static str,
    selected: Rgb,
    theme: &Theme,
    apply: impl Fn(&mut SubtitleStyle, Rgb) + Copy + 'static,
) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .gap_1()
        .children(
            SUBTITLE_COLORS
                .iter()
                .enumerate()
                .map(|(index, &(color, _))| {
                    div()
                        .id((id, index))
                        .flex_none()
                        .size(SWATCH)
                        .p(SWATCH_PADDING)
                        .rounded_full()
                        .border_2()
                        .border_color(if color == selected {
                            theme.accent
                        } else {
                            transparent_black()
                        })
                        .cursor_pointer()
                        .child(
                            div()
                                .size_full()
                                .rounded_full()
                                .border_1()
                                .border_color(theme.border)
                                .bg(rgb(color.0)),
                        )
                        .on_click(move |_, _, cx| {
                            update_settings(cx, true, |s| apply(&mut s.subtitles, color))
                        })
                }),
        )
}

const PREVIEW_TEXT: &str = r"Subtitles look like this,\N{\i1}and like this in italics.{\i0}";

/// A 16:9 tile with a letterboxed picture and a cue on it, drawn the way a
/// player draws its subtitles.
fn subtitle_preview(style: &SubtitleStyle, theme: &Theme) -> AnyElement {
    let cues = parse_ass(PREVIEW_TEXT)
        .map(|text| {
            Arc::new(CueContent {
                texts: vec![text],
                bitmaps: Vec::new(),
            })
        })
        .into_iter()
        .collect();
    div()
        .flex()
        .justify_center()
        .py_3()
        .superellipse(radius::CARD)
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            div()
                .relative()
                .flex_none()
                .w(PREVIEW_SIZE.width)
                .h(PREVIEW_SIZE.height)
                .superellipse(radius::CONTROL)
                .overflow_hidden()
                .bg(black())
                .child(
                    canvas(
                        |_, _, _| {},
                        |bounds, (), window, _| paint_picture(bounds, window),
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .child(subtitles::overlay(
                    cues,
                    subtitles::Layout {
                        video: Some(PREVIEW_PICTURE),
                        cover: false,
                        controls_height: px(0.),
                        controls_pinned: false,
                        mouse_in_window: false,
                        font: theme.font.clone(),
                        style: style.clone(),
                    },
                )),
        )
        .into_any_element()
}

/// A dusk sky for the preview, bright near the bottom where the subtitles
/// go, so the edge settings show.
fn paint_picture(bounds: Bounds<Pixels>, window: &mut Window) {
    let picture = subtitles::picture_bounds(bounds, Some(PREVIEW_PICTURE), false);
    window.paint_quad(fill(
        picture,
        linear_gradient(
            180.,
            linear_color_stop(rgb(PREVIEW_SKY.0), 0.),
            linear_color_stop(rgb(PREVIEW_SKY.1), 1.),
        ),
    ));
    let sun = picture.size.height * PREVIEW_SUN_SIZE;
    let center = point(
        picture.origin.x + picture.size.width * PREVIEW_SUN_CENTER.x,
        picture.origin.y + picture.size.height * PREVIEW_SUN_CENTER.y,
    );
    window.paint_quad(
        fill(
            Bounds::centered_at(center, size(sun, sun)),
            rgb(PREVIEW_SUN),
        )
        .corner_radii(sun / 2.),
    );
}

/// The font families installed, for the subtitle font menu, sorted. It
/// leaves out the system's hidden fonts, and the fonts gpui falls back to
/// on other systems, which it lists whether they are installed or not.
fn installed_fonts(cx: &App) -> Vec<SharedString> {
    const FALLBACKS: &[&str] = &[
        "Helvetica",
        "Segoe UI",
        "Ubuntu",
        "Adwaita Sans",
        "Cantarell",
        "Noto Sans",
        "DejaVu Sans",
        "Arial",
    ];
    let text_system = cx.text_system();
    let mut names: Vec<String> = text_system
        .all_font_names()
        .into_iter()
        .filter(|name| !name.starts_with('.'))
        .filter(|name| {
            !FALLBACKS.contains(&name.as_str()) || {
                // A missing font resolves to a fallback with another name.
                let id = text_system.resolve_font(&gpui::font(name.clone()));
                text_system
                    .get_font_for_id(id)
                    .is_some_and(|font| font.family.as_ref() == name)
            }
        })
        .collect();
    names.sort_by_cached_key(|name| name.to_lowercase());
    names.dedup();
    names.into_iter().map(SharedString::from).collect()
}

impl SettingsView {
    /// The button that shows the subtitle font and opens the menu of them.
    fn font_picker(
        &self,
        current: &str,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = if current.is_empty() {
            SharedString::from("System")
        } else {
            SharedString::from(current.to_owned())
        };
        let viewport = window.viewport_size();
        div()
            .relative()
            .flex_none()
            .child(
                div()
                    .id("subtitle-font")
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .w(FONT_BUTTON_WIDTH)
                    .h(controls::BUTTON_HEIGHT)
                    .pl_2p5()
                    .pr_1p5()
                    .superellipse(radius::CONTROL)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .text_sm()
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.surface_hover))
                    .on_click(cx.listener(Self::open_fonts))
                    .child(div().min_w_0().truncate().child(name))
                    .child(
                        Icon::new("chevron-up-down")
                            .size(controls::BUTTON_ICON)
                            .color(theme.text_muted),
                    ),
            )
            .when(self.fonts_open, |this| {
                this.child(
                    deferred(
                        anchored()
                            .position_mode(AnchoredPositionMode::Local)
                            .anchor(Corner::TopRight)
                            .position(point(FONT_BUTTON_WIDTH, FONT_MENU_TOP))
                            .snap_to_window_with_margin(menu::WINDOW_MARGIN)
                            .child(self.font_menu(current, theme, cx)),
                    )
                    .with_priority(2),
                )
                // Clicks anywhere else close the menu, and do nothing else.
                .child(
                    deferred(
                        anchored().position(point(px(0.), px(0.))).child(
                            div()
                                .id("fonts-backdrop")
                                .occlude()
                                .w(viewport.width)
                                .h(viewport.height)
                                .on_mouse_down(MouseButton::Left, cx.listener(Self::close_fonts))
                                .on_mouse_down(MouseButton::Right, cx.listener(Self::close_fonts)),
                        ),
                    )
                    .with_priority(1),
                )
            })
            .into_any_element()
    }

    /// "System" and every installed family, each with a sample in its font.
    fn font_menu(&self, current: &str, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let fonts = self.fonts.clone();
        let current = current.to_owned();
        let theme = theme.clone();
        let list = uniform_list(
            "fonts",
            fonts.len() + 1,
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|ix| {
                        let family = ix.checked_sub(1).map(|i| fonts[i].clone());
                        let checked =
                            family.as_ref().map_or("", |f| f.as_ref()) == current.as_str();
                        let sample = family.clone().unwrap_or_else(|| theme.font.clone());
                        div()
                            .id(ix)
                            .group("font-row")
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .h(menu::ROW_HEIGHT)
                            .mx_1()
                            .px_2()
                            .superellipse(radius::ITEM)
                            .cursor_pointer()
                            .hover(|style| style.bg(theme.accent).text_color(white()))
                            .child(
                                div()
                                    .flex_none()
                                    .w(FONT_CHECK_WIDTH)
                                    .when(checked, |this| this.child("✓")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(family.clone().unwrap_or_else(|| "System".into())),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .font_family(sample)
                                    .text_color(theme.text_muted)
                                    .group_hover("font-row", |style| style.text_color(white()))
                                    .child("Aa"),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.fonts_open = false;
                                let font =
                                    family.as_ref().map_or_else(String::new, |f| f.to_string());
                                update_settings(cx, true, |s| s.subtitles.font = font);
                                cx.notify();
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.font_scroll.clone())
        .h(FONT_MENU_HEIGHT)
        .py_1();
        Panel::new().occlude().w(FONT_MENU_WIDTH).child(list)
    }

    fn open_fonts(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.fonts.is_empty() {
            self.fonts = installed_fonts(cx).into();
        }
        let current = &cx.global::<Settings>().subtitles.font;
        let ix = if current.is_empty() {
            0
        } else {
            self.fonts
                .iter()
                .position(|font| font.as_ref() == current.as_str())
                .map_or(0, |i| i + 1)
        };
        self.font_scroll.scroll_to_item(ix, ScrollStrategy::Center);
        self.fonts_open = true;
        cx.notify();
    }

    fn close_fonts(&mut self, _: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.fonts_open = false;
        cx.stop_propagation();
        cx.notify();
    }

    fn subtitle_groups(
        &self,
        style: &SubtitleStyle,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        vec![
            group(
                Some("Text"),
                theme,
                vec![
                    row(
                        "Font",
                        None,
                        theme,
                        self.font_picker(&style.font, theme, window, cx),
                    ),
                    row(
                        "Weight",
                        None,
                        theme,
                        Segmented::new(
                            "subtitle-weight",
                            &[
                                (SubtitleWeight::Regular, "Regular"),
                                (SubtitleWeight::Medium, "Medium"),
                                (SubtitleWeight::Bold, "Bold"),
                            ],
                            style.weight,
                        )
                        .on_select(|&v, _, cx| {
                            update_settings(cx, true, |s| s.subtitles.weight = v)
                        }),
                    ),
                    row(
                        "Size",
                        Some("Grows with the picture".into()),
                        theme,
                        range_slider(
                            "subtitle-size",
                            style.size,
                            SUBTITLE_SIZE_RANGE,
                            20.0,
                            self.subtitle_size_drag.clone(),
                            theme,
                            |s, v| s.subtitles.size = v,
                        ),
                    ),
                    row(
                        "Color",
                        None,
                        theme,
                        swatches("subtitle-color", style.color, theme, |s, c| s.color = c),
                    ),
                ],
            ),
            group(
                Some("Edges and background"),
                theme,
                vec![
                    row(
                        "Edge",
                        None,
                        theme,
                        Segmented::new(
                            "subtitle-edge",
                            &[
                                (SubtitleEdge::None, "None"),
                                (SubtitleEdge::Outline, "Outline"),
                                (SubtitleEdge::Shadow, "Shadow"),
                            ],
                            style.edge,
                        )
                        .on_select(|&v, _, cx| update_settings(cx, true, |s| s.subtitles.edge = v)),
                    ),
                    row(
                        "Edge color",
                        None,
                        theme,
                        swatches("subtitle-edge-color", style.edge_color, theme, |s, c| {
                            s.edge_color = c
                        }),
                    ),
                    row(
                        "Background",
                        Some("A box behind each line".into()),
                        theme,
                        percent_slider(
                            "subtitle-background",
                            style.background_opacity,
                            self.subtitle_background_drag.clone(),
                            theme,
                            |s, v| s.subtitles.background_opacity = v,
                        ),
                    ),
                    row(
                        "Background color",
                        None,
                        theme,
                        swatches(
                            "subtitle-background-color",
                            style.background,
                            theme,
                            |s, c| s.background = c,
                        ),
                    ),
                ],
            ),
            group(
                Some("Position"),
                theme,
                vec![
                    row(
                        "Place on",
                        Some(
                            "Tile lets subtitles sit in the black bars around a letterboxed \
                             picture"
                                .into(),
                        ),
                        theme,
                        Segmented::new(
                            "subtitle-area",
                            &[
                                (SubtitleArea::Picture, "Picture"),
                                (SubtitleArea::Tile, "Tile"),
                            ],
                            style.area,
                        )
                        .on_select(|&v, _, cx| update_settings(cx, true, |s| s.subtitles.area = v)),
                    ),
                    row(
                        "Height",
                        Some("Above the bottom. Subtitles move up while the controls show.".into()),
                        theme,
                        range_slider(
                            "subtitle-position",
                            style.position,
                            SUBTITLE_POSITION_RANGE,
                            100.0,
                            self.subtitle_position_drag.clone(),
                            theme,
                            |s, v| s.subtitles.position = v,
                        ),
                    ),
                ],
            ),
        ]
    }
}

impl SettingsView {
    /// A group of commands for each part of playback, and their keys.
    fn hotkey_groups(
        &self,
        hotkeys: &Hotkeys,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let intro = group(
            None,
            theme,
            vec![row(
                "IINA's shortcuts",
                Some(
                    "Shortcuts go to the tile last clicked. Click + and press keys to add \
                     one; menu shortcuts such as ⌘O stay as they are."
                        .into(),
                ),
                theme,
                Button::new("restore-hotkeys", "Restore").on_click(|_, _, cx| {
                    update_settings(cx, true, |s| s.hotkeys = Hotkeys::default())
                }),
            )],
        );
        let mut groups = vec![intro];
        for kind in Group::ALL {
            let rows = Command::ALL
                .into_iter()
                .filter(|command| command.group() == kind)
                .map(|command| {
                    let note = self
                        .hotkey_note
                        .as_ref()
                        .filter(|(noted, _)| *noted == command)
                        .map(|(_, note)| note.clone());
                    row(
                        command.title(),
                        note,
                        theme,
                        self.hotkey_keys(command, hotkeys.keys(command), theme, cx),
                    )
                })
                .collect();
            groups.push(group(Some(kind.title()), theme, rows));
        }
        groups
    }

    /// A command's keys, each with a button to remove it, and a button to
    /// add one (or a prompt while waiting for it).
    fn hotkey_keys(
        &self,
        command: Command,
        keys: &[String],
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chip = |id: SharedString| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .h(HOTKEY_CHIP_HEIGHT)
                .px_2()
                .superellipse(radius::ITEM)
                .border_1()
                .text_sm()
        };
        let recording = self.recording == Some(command);
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .justify_end()
            .items_center()
            .gap_1()
            .max_w(HOTKEY_KEYS_MAX_WIDTH)
            .children(keys.iter().map(|key| {
                let removed = key.clone();
                chip(format!("{}-{key}", command.id()).into())
                    .border_color(theme.border)
                    .bg(theme.background)
                    .child(hotkeys::display(key))
                    .child(
                        div()
                            .id("remove")
                            .flex_none()
                            .cursor_pointer()
                            .superellipse(radius::SMALL)
                            .hover(|style| style.bg(theme.surface_hover))
                            .child(
                                Icon::new("close")
                                    .size(HOTKEY_REMOVE_ICON)
                                    .color(theme.text_muted),
                            )
                            .on_click(move |_, _, cx| {
                                update_settings(cx, true, |s| s.hotkeys.remove(command, &removed))
                            }),
                    )
            }))
            .child(if recording {
                chip(format!("{}-recording", command.id()).into())
                    .border_color(theme.accent)
                    .text_color(theme.accent)
                    .cursor_pointer()
                    .child("Press keys… (⎋ cancels)")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.recording = None;
                        cx.notify();
                    }))
            } else {
                chip(format!("{}-add", command.id()).into())
                    .border_color(theme.border)
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.surface_hover))
                    .child(
                        Icon::new("plus")
                            .size(HOTKEY_ADD_ICON)
                            .color(theme.text_muted),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.recording = Some(command);
                        this.hotkey_note = None;
                        window.focus(&this.focus_handle);
                        cx.notify();
                    }))
            })
            .into_any_element()
    }

    /// While waiting for a shortcut, the keys pressed become it. Escape
    /// cancels; the app's own shortcuts (⌘O and so on) never get here.
    fn record_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(command) = self.recording else {
            return;
        };
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let pressed = &event.keystroke;
        if pressed.key == "escape" && !pressed.modifiers.modified() {
            self.recording = None;
            cx.notify();
            return;
        }
        let key = Keystroke {
            modifiers: pressed.modifiers,
            key: pressed.key.clone(),
            key_char: None,
        }
        .unparse();
        let mut taken = None;
        update_settings(cx, true, |s| taken = s.hotkeys.add(command, &key));
        self.hotkey_note = taken.map(|from| {
            let note = format!("{} was for “{}”", hotkeys::display(&key), from.title());
            (command, SharedString::from(note))
        });
        self.recording = None;
        cx.notify();
    }
}

impl SettingsView {
    /// A section's pane: its groups of rows.
    fn groups(
        &self,
        section: Section,
        theme: &Theme,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let s = cx.global::<Settings>().clone();
        match section {
            Section::Playback => vec![group(
                None,
                theme,
                vec![
                    row(
                        "Play files when opened",
                        None,
                        theme,
                        Switch::new("autoplay", s.autoplay)
                            .on_toggle(|&on, _, cx| update_settings(cx, true, |s| s.autoplay = on)),
                    ),
                    row(
                        "Loop new files",
                        None,
                        theme,
                        Switch::new("loop", s.loop_by_default).on_toggle(|&on, _, cx| {
                            update_settings(cx, true, |s| s.loop_by_default = on)
                        }),
                    ),
                    row(
                        "Volume for new files",
                        None,
                        theme,
                        percent_slider(
                            "default-volume",
                            s.default_volume,
                            self.default_drag.clone(),
                            theme,
                            |s, v| s.default_volume = v,
                        ),
                    ),
                    row(
                        "Seek step",
                        Some("How far the seek shortcuts move, ← and → at first".into()),
                        theme,
                        Segmented::new(
                            "seek-step",
                            &[(5, "5 s"), (10, "10 s"), (30, "30 s"), (60, "60 s")],
                            s.seek_step_seconds,
                        )
                        .on_select(|&v, _, cx| {
                            update_settings(cx, true, |s| s.seek_step_seconds = v)
                        }),
                    ),
                ],
            )],
            Section::Audio => vec![group(
                None,
                theme,
                vec![
                    row(
                        "Master volume",
                        None,
                        theme,
                        percent_slider(
                            "master-volume",
                            s.master_volume,
                            self.master_drag.clone(),
                            theme,
                            |s, v| s.master_volume = v,
                        ),
                    ),
                    row(
                        "Output",
                        None,
                        theme,
                        div()
                            .text_sm()
                            .text_color(theme.text_muted)
                            .max_w(OUTPUT_MAX_WIDTH)
                            .truncate()
                            .child(AudioEngine::device_description(cx)),
                    ),
                ],
            )],
            Section::Video => {
                let [sdr_gamma, hdr_gamma] = s.gamma();
                vec![
                    group(
                        Some("Picture"),
                        theme,
                        vec![
                            row(
                                "Scaling",
                                None,
                                theme,
                                Segmented::new(
                                    "fit",
                                    &[(VideoFit::Contain, "Fit"), (VideoFit::Cover, "Fill")],
                                    s.video_fit,
                                )
                                .on_select(|&v, _, cx| {
                                    update_settings(cx, true, |s| s.video_fit = v)
                                }),
                            ),
                            row(
                                "SDR gamma",
                                Some(
                                    "Above 1 brightens shadows and midtones, below 1 darkens \
                                     them. For every open video."
                                        .into(),
                                ),
                                theme,
                                gamma_slider(
                                    "sdr-gamma",
                                    sdr_gamma,
                                    self.sdr_gamma_drag.clone(),
                                    theme,
                                    |s, v| s.sdr_gamma = v,
                                ),
                            ),
                            row(
                                "HDR gamma",
                                Some("The same for HDR10, HLG and Dolby Vision video.".into()),
                                theme,
                                gamma_slider(
                                    "hdr-gamma",
                                    hdr_gamma,
                                    self.hdr_gamma_drag.clone(),
                                    theme,
                                    |s, v| s.hdr_gamma = v,
                                ),
                            ),
                        ],
                    ),
                    group(
                        Some("Decoding"),
                        theme,
                        vec![
                            row(
                                "Resolution limit",
                                Some(
                                    "For files opened from now on. Lower limits save CPU and \
                                     memory with many videos."
                                        .into(),
                                ),
                                theme,
                                Segmented::new(
                                    "max-resolution",
                                    &[
                                        (MaxResolution::Original, "Full"),
                                        (MaxResolution::P2160, "2160p"),
                                        (MaxResolution::P1080, "1080p"),
                                        (MaxResolution::P720, "720p"),
                                        (MaxResolution::P480, "480p"),
                                    ],
                                    s.max_resolution,
                                )
                                .on_select(|&v, _, cx| {
                                    update_settings(cx, true, |s| s.max_resolution = v)
                                }),
                            ),
                            row(
                                "Hardware decoding",
                                Some(
                                    if cfg!(target_os = "macos") {
                                        "Decode on the GPU with VideoToolbox when it supports \
                                         the video. For files opened from now on."
                                    } else {
                                        "Decode on the GPU with VA-API, NVDEC or VDPAU when one \
                                         supports the video. For files opened from now on."
                                    }
                                    .into(),
                                ),
                                theme,
                                Switch::new("hardware", s.hardware_decoding).on_toggle(
                                    |&on, _, cx| {
                                        update_settings(cx, true, |s| s.hardware_decoding = on)
                                    },
                                ),
                            ),
                        ],
                    ),
                ]
            }
            Section::Subtitles => self.subtitle_groups(&s.subtitles.clamped(), theme, window, cx),
            Section::Hotkeys => self.hotkey_groups(&s.hotkeys, theme, cx),
            Section::Appearance => vec![
                group(
                    None,
                    theme,
                    vec![row(
                        "Theme",
                        None,
                        theme,
                        Segmented::new(
                            "theme",
                            &[
                                (ThemeMode::System, "System"),
                                (ThemeMode::Dark, "Dark"),
                                (ThemeMode::Light, "Light"),
                            ],
                            s.theme,
                        )
                        .on_select(|&v, _, cx| update_settings(cx, true, |s| s.theme = v)),
                    )],
                ),
                group(
                    Some("Windows"),
                    theme,
                    vec![
                        row(
                            "Columns",
                            None,
                            theme,
                            Segmented::new(
                                "columns",
                                &[(0, "Auto"), (1, "1"), (2, "2"), (3, "3"), (4, "4")],
                                s.columns,
                            )
                            .on_select(|&v, _, cx| update_settings(cx, true, |s| s.columns = v)),
                        ),
                        row(
                            "Player controls",
                            None,
                            theme,
                            Segmented::new(
                                "controls",
                                &[
                                    (ControlsVisibility::OnHover, "On hover"),
                                    (ControlsVisibility::Always, "Always"),
                                ],
                                s.controls,
                            )
                            .on_select(|&v, _, cx| update_settings(cx, true, |s| s.controls = v)),
                        ),
                        row(
                            "Show file names",
                            None,
                            theme,
                            Switch::new("titles", s.show_titles).on_toggle(|&on, _, cx| {
                                update_settings(cx, true, |s| s.show_titles = on)
                            }),
                        ),
                    ],
                ),
            ],
        }
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
            .children(Section::ALL.map(|section| {
                let selected = section == self.section;
                div()
                    .id(section.title())
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
                        this.section = section;
                        this.fonts_open = false;
                        this.recording = None;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_none()
                            .size(BADGE)
                            .superellipse(radius::ITEM)
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(section.color())
                            .child(Icon::new(section.icon()).size(BADGE_ICON).color(white())),
                    )
                    .child(section.title())
            }))
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let path_hint = Settings::path()
            .map(|p| format!("Saved to {}", p.display()))
            .unwrap_or_else(|| "Settings can't be saved on this system".into());
        let section = self.section;

        let pane = div()
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
                    .text_base()
                    .font_weight(FontWeight::BOLD)
                    .child(section.title()),
            )
            // The preview stays in view while the settings under it scroll.
            .when(section == Section::Subtitles, |this| {
                let style = cx.global::<Settings>().subtitles.clamped();
                this.child(
                    div()
                        .flex_none()
                        .px_5()
                        .pt_2()
                        .pb_1()
                        .child(subtitle_preview(&style, &theme)),
                )
            })
            .child(
                div()
                    // Each pane scrolls on its own and opens at the top.
                    .id(("settings-pane", section as usize))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .px_5()
                    .pt_2()
                    .pb_5()
                    .children(self.groups(section, &theme, window, cx)),
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
            .on_key_down(cx.listener(Self::record_key))
            .size_full()
            .flex()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .child(self.render_sidebar(&theme, cx))
            .child(pane)
    }
}
