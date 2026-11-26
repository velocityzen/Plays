//! Subtitles: how they look and where they go, with a preview above that
//! stays in view, and the languages to show them in.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, Corner, MouseDownEvent, Pixels, ScrollStrategy,
    SharedString, UniformListScrollHandle, Window, black, canvas, div, fill, linear_color_stop,
    linear_gradient, point, px, rgb, size, transparent_black, uniform_list,
};

use super::form::{group, percent_slider, range_slider, row, set};
use super::languages::{List, languages_group};
use super::{Pane, SettingsView};
use crate::media::Orientation;
use crate::media::subtitles::{CueContent, parse_ass};
use crate::settings::{
    Rgb, SUBTITLE_COLORS, SUBTITLE_POSITION_RANGE, SUBTITLE_SIZE_RANGE, Settings, SubtitleStyle,
    fields,
};
use crate::ui::components::{
    Button, Card, ComponentSize, MenuRow, Panel, Popover, Segmented, Switch,
};
use crate::ui::framing::{self, Adjustments, Framing};
use crate::ui::subtitles;
use crate::ui::theme::settings::{
    FONT_BUTTON_WIDTH, FONT_MENU_HEIGHT, FONT_MENU_TOP, FONT_MENU_WIDTH, PREVIEW_PICTURE,
    PREVIEW_SIZE, PREVIEW_SKY, PREVIEW_SUN, PREVIEW_SUN_CENTER, PREVIEW_SUN_SIZE, SUBTITLES_COLOR,
    SWATCH, SWATCH_PADDING,
};
use crate::ui::theme::{Theme, radius};

pub(super) struct SubtitlesPane;

/// The subtitle font menu.
#[derive(Default)]
pub(super) struct FontMenu {
    open: bool,
    /// The installed font families, listed the first time the menu opens.
    fonts: Rc<[SharedString]>,
    scroll: UniformListScrollHandle,
}

impl Pane for SubtitlesPane {
    fn name(&self) -> &'static str {
        "Subtitles"
    }

    fn icon(&self) -> &'static str {
        "subtitles"
    }

    fn color(&self) -> u32 {
        SUBTITLES_COLOR
    }

    fn content(
        &self,
        view: &mut SettingsView,
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let style = s.subtitles.clamped();
        vec![
            text_group(view, &style, theme, cx),
            edges_group(view, &style, theme),
            position_group(view, &style, theme),
            languages_group(view, List::Subtitles, s, theme, cx),
        ]
    }

    /// The preview stays in view while the settings under it scroll.
    fn header(&self, s: &Settings, theme: &Theme) -> Option<AnyElement> {
        Some(preview(&s.subtitles.clamped(), theme))
    }

    fn leave(&self, view: &mut SettingsView) {
        view.font_menu.open = false;
        view.language_menu = None;
    }
}

/// The subtitles' font, and its weight, size and color.
fn text_group(
    view: &mut SettingsView,
    style: &SubtitleStyle,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    group(
        Some("Text"),
        theme,
        vec![
            row(
                "File's own styles",
                Some(
                    "ASS and SSA subtitles in the fonts, colors and places their files \
                     give them, sized as below"
                        .into(),
                ),
                theme,
                Switch::new("subtitle-file-styles", style.file_styles)
                    .on_toggle(|&on, _, cx| set("subtitles.file_styles", on, cx)),
            ),
            row(
                "Font",
                None,
                theme,
                font_picker(view, &style.font, theme, cx),
            ),
            row(
                "Weight",
                None,
                theme,
                Segmented::new("subtitle-weight", &fields::WEIGHTS, style.weight)
                    .on_select(|&v, _, cx| set("subtitles.weight", v, cx)),
            ),
            row(
                "Size",
                Some("Grows with the picture".into()),
                theme,
                range_slider(
                    view,
                    "subtitle-size",
                    "subtitles.size",
                    style.size,
                    SUBTITLE_SIZE_RANGE,
                    20.0,
                    theme,
                ),
            ),
            row(
                "Color",
                None,
                theme,
                swatches("subtitle-color", "subtitles.color", style.color, theme),
            ),
        ],
    )
}

/// The edge drawn around the letters, and the box behind each line.
fn edges_group(view: &mut SettingsView, style: &SubtitleStyle, theme: &Theme) -> AnyElement {
    group(
        Some("Edges and background"),
        theme,
        vec![
            row(
                "Edge",
                None,
                theme,
                Segmented::new("subtitle-edge", &fields::EDGES, style.edge)
                    .on_select(|&v, _, cx| set("subtitles.edge", v, cx)),
            ),
            row(
                "Edge color",
                None,
                theme,
                swatches(
                    "subtitle-edge-color",
                    "subtitles.edge_color",
                    style.edge_color,
                    theme,
                ),
            ),
            row(
                "Background",
                Some("A box behind each line".into()),
                theme,
                percent_slider(
                    view,
                    "subtitle-background",
                    "subtitles.background_opacity",
                    style.background_opacity,
                    theme,
                ),
            ),
            row(
                "Background color",
                None,
                theme,
                swatches(
                    "subtitle-background-color",
                    "subtitles.background",
                    style.background,
                    theme,
                ),
            ),
        ],
    )
}

/// Where the subtitles go on the block.
fn position_group(view: &mut SettingsView, style: &SubtitleStyle, theme: &Theme) -> AnyElement {
    group(
        Some("Position"),
        theme,
        vec![
            row(
                "Place on",
                Some(
                    "Block lets subtitles sit in the black bars around a letterboxed \
                     picture"
                        .into(),
                ),
                theme,
                Segmented::new("subtitle-area", &fields::AREAS, style.area)
                    .on_select(|&v, _, cx| set("subtitles.area", v, cx)),
            ),
            row(
                "Height",
                Some("Above the bottom. Subtitles move up while the controls show.".into()),
                theme,
                range_slider(
                    view,
                    "subtitle-position",
                    "subtitles.position",
                    style.position,
                    SUBTITLE_POSITION_RANGE,
                    100.0,
                    theme,
                ),
            ),
        ],
    )
}

/// A row of colors to pick one from, for the subtitle style's color that
/// `apply` sets.
fn swatches(id: &'static str, key: &'static str, selected: Rgb, theme: &Theme) -> AnyElement {
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
                        .on_click(move |_, _, cx| set(key, color, cx))
                }),
        )
        .into_any_element()
}

const PREVIEW_TEXT: &str = r"Subtitles look like this,\N{\i1}and like this in italics.{\i0}";

/// A 16:9 block with a letterboxed picture and a cue on it, drawn the way a
/// player draws its subtitles.
fn preview(style: &SubtitleStyle, theme: &Theme) -> AnyElement {
    let cues = parse_ass(PREVIEW_TEXT)
        .map(|text| {
            Arc::new(CueContent {
                texts: vec![text],
                bitmaps: Vec::new(),
            })
        })
        .into_iter()
        .collect();
    Card::new()
        .flex()
        .justify_center()
        .py_3()
        .child(
            div()
                .relative()
                .flex_none()
                .w(PREVIEW_SIZE.width)
                .h(PREVIEW_SIZE.height)
                .superellipse(ComponentSize::Medium.pick(radius::CONTROL))
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
                        framing: Framing::new(
                            PREVIEW_PICTURE,
                            Orientation::UPRIGHT,
                            &Adjustments::default(),
                        ),
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
    let (width, height) = PREVIEW_PICTURE;
    let picture = framing::fit(bounds, (width as f32, height as f32), false);
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

/// The button that shows the subtitle font and opens the menu of them.
fn font_picker(
    view: &SettingsView,
    current: &str,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let name = if current.is_empty() {
        SharedString::from("System")
    } else {
        SharedString::from(current.to_owned())
    };
    Popover::new(
        "fonts-backdrop",
        Button::new("subtitle-font", name)
            .icon_right("chevron-up-down")
            .w(FONT_BUTTON_WIDTH)
            .on_click(cx.listener(open_fonts)),
    )
    .anchor(Corner::TopRight, point(FONT_BUTTON_WIDTH, FONT_MENU_TOP))
    .when(view.font_menu.open, |this| {
        this.menu(font_menu(view, current, theme, cx))
    })
    .on_dismiss(cx.listener(close_fonts))
    .into_any_element()
}

/// "System" and every installed family, each with a sample in its font.
fn font_menu(
    view: &SettingsView,
    current: &str,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement + use<> {
    let fonts = view.font_menu.fonts.clone();
    let current = current.to_owned();
    let theme = theme.clone();
    let list = uniform_list(
        "fonts",
        fonts.len() + 1,
        cx.processor(move |_, range: Range<usize>, _, cx| {
            range
                .map(|ix| {
                    let family = ix.checked_sub(1).map(|i| fonts[i].clone());
                    let checked = family.as_ref().map_or("", |f| f.as_ref()) == current.as_str();
                    let sample = family.clone().unwrap_or_else(|| theme.font.clone());
                    MenuRow::new(ix, family.clone().unwrap_or_else(|| "System".into()))
                        .checked(checked)
                        .detail(div().font_family(sample).child("Aa"))
                        .mx_1()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.font_menu.open = false;
                            let font = family.as_ref().map_or_else(String::new, |f| f.to_string());
                            set("subtitles.font", font, cx);
                            cx.notify();
                        }))
                })
                .collect::<Vec<_>>()
        }),
    )
    .track_scroll(view.font_menu.scroll.clone())
    .h(FONT_MENU_HEIGHT)
    .py_1();
    Panel::new().occlude().w(FONT_MENU_WIDTH).child(list)
}

fn open_fonts(
    view: &mut SettingsView,
    _: &ClickEvent,
    _: &mut Window,
    cx: &mut Context<SettingsView>,
) {
    let menu = &mut view.font_menu;
    if menu.fonts.is_empty() {
        menu.fonts = installed_fonts(cx).into();
    }
    let current = &cx.global::<Settings>().subtitles.font;
    let ix = if current.is_empty() {
        0
    } else {
        menu.fonts
            .iter()
            .position(|font| font.as_ref() == current.as_str())
            .map_or(0, |i| i + 1)
    };
    menu.scroll.scroll_to_item(ix, ScrollStrategy::Center);
    menu.open = true;
    cx.notify();
}

fn close_fonts(
    view: &mut SettingsView,
    _: &MouseDownEvent,
    _: &mut Window,
    cx: &mut Context<SettingsView>,
) {
    view.font_menu.open = false;
    cx.notify();
}

/// The font families installed, for the subtitle font menu, sorted. It
/// leaves out the system's hidden fonts, and the fonts gpui falls back to
/// on other systems, which it lists whether they are installed or not.
pub fn installed_fonts(cx: &App) -> Vec<SharedString> {
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
