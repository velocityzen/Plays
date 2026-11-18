//! Subtitles: how they look and where they go, with a preview above that
//! stays in view.

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnchoredPositionMode, AnyElement, App, Bounds, ClickEvent, Context, Corner, MouseButton,
    MouseDownEvent, Pixels, ScrollStrategy, SharedString, UniformListScrollHandle, Window,
    anchored, black, canvas, deferred, div, fill, linear_color_stop, linear_gradient, point, px,
    rgb, size, transparent_black, uniform_list, white,
};

use super::form::{group, percent_slider, range_slider, row, update_settings};
use super::{Pane, SettingsView};
use crate::media::subtitles::{CueContent, parse_ass};
use crate::settings::{
    Rgb, SUBTITLE_COLORS, SUBTITLE_POSITION_RANGE, SUBTITLE_SIZE_RANGE, Settings, SubtitleArea,
    SubtitleEdge, SubtitleStyle, SubtitleWeight,
};
use crate::ui::components::{Icon, Panel, Segmented};
use crate::ui::subtitles;
use crate::ui::theme::settings::{
    FONT_BUTTON_WIDTH, FONT_CHECK_WIDTH, FONT_MENU_HEIGHT, FONT_MENU_TOP, FONT_MENU_WIDTH,
    PREVIEW_PICTURE, PREVIEW_SIZE, PREVIEW_SKY, PREVIEW_SUN, PREVIEW_SUN_CENTER, PREVIEW_SUN_SIZE,
    SUBTITLES_COLOR, SWATCH, SWATCH_PADDING,
};
use crate::ui::theme::{Theme, controls, menu, radius};

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
        window: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let style = s.subtitles.clamped();
        vec![
            group(
                Some("Text"),
                theme,
                vec![
                    row(
                        "Font",
                        None,
                        theme,
                        font_picker(view, &style.font, theme, window, cx),
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
                            view,
                            "subtitle-size",
                            style.size,
                            SUBTITLE_SIZE_RANGE,
                            20.0,
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
                            view,
                            "subtitle-background",
                            style.background_opacity,
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
                            view,
                            "subtitle-position",
                            style.position,
                            SUBTITLE_POSITION_RANGE,
                            100.0,
                            theme,
                            |s, v| s.subtitles.position = v,
                        ),
                    ),
                ],
            ),
        ]
    }

    /// The preview stays in view while the settings under it scroll.
    fn header(&self, s: &Settings, theme: &Theme) -> Option<AnyElement> {
        Some(preview(&s.subtitles.clamped(), theme))
    }

    fn leave(&self, view: &mut SettingsView) {
        view.font_menu.open = false;
    }
}

/// A row of colors to pick one from, for the subtitle style's color that
/// `apply` sets.
fn swatches(
    id: &'static str,
    selected: Rgb,
    theme: &Theme,
    apply: impl Fn(&mut SubtitleStyle, Rgb) + Copy + 'static,
) -> AnyElement {
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
        .into_any_element()
}

const PREVIEW_TEXT: &str = r"Subtitles look like this,\N{\i1}and like this in italics.{\i0}";

/// A 16:9 tile with a letterboxed picture and a cue on it, drawn the way a
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

/// The button that shows the subtitle font and opens the menu of them.
fn font_picker(
    view: &SettingsView,
    current: &str,
    theme: &Theme,
    window: &Window,
    cx: &mut Context<SettingsView>,
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
                .on_click(cx.listener(open_fonts))
                .child(div().min_w_0().truncate().child(name))
                .child(
                    Icon::new("chevron-up-down")
                        .size(controls::BUTTON_ICON)
                        .color(theme.text_muted),
                ),
        )
        .when(view.font_menu.open, |this| {
            this.child(
                deferred(
                    anchored()
                        .position_mode(AnchoredPositionMode::Local)
                        .anchor(Corner::TopRight)
                        .position(point(FONT_BUTTON_WIDTH, FONT_MENU_TOP))
                        .snap_to_window_with_margin(menu::WINDOW_MARGIN)
                        .child(font_menu(view, current, theme, cx)),
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
                            .on_mouse_down(MouseButton::Left, cx.listener(close_fonts))
                            .on_mouse_down(MouseButton::Right, cx.listener(close_fonts)),
                    ),
                )
                .with_priority(1),
            )
        })
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
                            this.font_menu.open = false;
                            let font = family.as_ref().map_or_else(String::new, |f| f.to_string());
                            update_settings(cx, true, |s| s.subtitles.font = font);
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
    cx.stop_propagation();
    cx.notify();
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
