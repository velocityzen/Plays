//! The component gallery: every component in each of its sizes and states,
//! side by side, to check them by eye. Debug builds only, opened from the
//! Developer menu. The switches, segments, sliders and menu rows work, so
//! hovering, pressing and dragging can be seen too.

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, Entity, Global, Hsla, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowHandle, WindowOptions, black, div, rgb,
};

use super::components::{
    Badge, Button, ButtonVariant, Card, ComponentSize, Icon, MenuRow, Meter, Panel, Segmented,
    Sizable, Slider, SliderEvent, Switch,
};
use super::displays;
use super::text_input::TextInput;
use super::theme::Theme;
use super::theme::gallery::{
    BADGE_COLOR, FILL_WIDTH, LABEL_WIDTH, SIZE_WIDTH, WINDOW_MIN_SIZE, WINDOW_SIZE,
};
use super::theme::layout::TRAFFIC_LIGHTS;
use super::theme::text::TextStyle;
use super::workspace::float_with_players;

/// Every size, in the order each row shows them.
const SIZES: [(ComponentSize, &str); 3] = [
    (ComponentSize::Small, "Small"),
    (ComponentSize::Medium, "Medium"),
    (ComponentSize::Large, "Large"),
];

/// The button variants with a label.
const VARIANTS: [(ButtonVariant, &str); 6] = [
    (ButtonVariant::Normal, "Normal"),
    (ButtonVariant::Primary, "Primary"),
    (ButtonVariant::Secondary, "Secondary"),
    (ButtonVariant::Danger, "Danger"),
    (ButtonVariant::Ghost, "Ghost"),
    (ButtonVariant::Link, "Link"),
];

const SEGMENTS: [(usize, &str); 3] = [(0, "One"), (1, "Two"), (2, "Three")];

/// A component as one of the sizes draws it; handed the size and a name
/// to tell its ids apart by.
type Draw<'a> = &'a dyn Fn(ComponentSize, &str) -> AnyElement;

pub struct GalleryView {
    /// The switches' state.
    on: bool,
    /// The segment picked.
    segment: usize,
    /// The sliders' and the meters' value.
    value: f32,
    dragging: Rc<Cell<bool>>,
    /// The menu row checked.
    checked: usize,
    /// A text field in each size.
    inputs: [Entity<TextInput>; 3],
}

/// The gallery window, once it has been opened.
struct GalleryWindow(WindowHandle<GalleryView>);
impl Global for GalleryWindow {}

impl GalleryView {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            on: true,
            segment: 0,
            value: 0.4,
            dragging: Rc::default(),
            checked: 0,
            inputs: SIZES
                .map(|(size, _)| cx.new(|cx| TextInput::new("Placeholder", cx).with_size(size))),
        }
    }

    /// Bring the gallery to the front, opening it if it isn't open, on the
    /// display in use (`displays::in_use`).
    pub fn open_window(cx: &mut App) {
        if let Some(handle) = cx.try_global::<GalleryWindow>().map(|open| open.0)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let mac = cfg!(target_os = "macos");
        let display = displays::in_use(cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    display,
                    WINDOW_SIZE,
                    cx,
                ))),
                display_id: display,
                titlebar: Some(TitlebarOptions {
                    title: Some("Component Gallery".into()),
                    appears_transparent: mac,
                    traffic_light_position: mac.then_some(TRAFFIC_LIGHTS),
                }),
                window_min_size: Some(WINDOW_MIN_SIZE),
                app_id: Some("plays".into()),
                focus: true,
                ..Default::default()
            },
            |_, cx| cx.new(Self::new),
        );
        match opened {
            Ok(handle) => {
                cx.set_global(GalleryWindow(handle));
                float_with_players(cx);
            }
            Err(err) => log::error!("could not open the component gallery: {err:#}"),
        }
    }

    fn buttons(theme: &Theme) -> AnyElement {
        let mut rows = vec![];
        for (variant, name) in VARIANTS {
            for (state, selected, disabled) in [
                ("", false, false),
                ("selected", true, false),
                ("disabled", false, true),
            ] {
                let label = if state.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}, {state}")
                };
                rows.push(row(&label, theme, &|size, id| {
                    Button::new(SharedString::from(id.to_owned()), name)
                        .variant(variant)
                        .with_size(size)
                        .selected(selected)
                        .disabled(disabled)
                        .into_any_element()
                }));
            }
        }
        rows.extend([
            row("Icon left", theme, &|size, id| {
                Button::new(SharedString::from(format!("{id}-icon-left")), "Open")
                    .icon_left("file")
                    .with_size(size)
                    .into_any_element()
            }),
            row("Icon right", theme, &|size, id| {
                Button::new(SharedString::from(format!("{id}-icon-right")), "Choose")
                    .icon_right("chevron-down")
                    .with_size(size)
                    .into_any_element()
            }),
            row("Shortcut", theme, &|size, id| {
                Button::new(SharedString::from(format!("{id}-shortcut")), "Open")
                    .shortcut("⌘O")
                    .with_size(size)
                    .into_any_element()
            }),
            row("Primary, shortcut", theme, &|size, id| {
                Button::new(SharedString::from(format!("{id}-primary-shortcut")), "Open")
                    .primary()
                    .shortcut("⌘O")
                    .with_size(size)
                    .into_any_element()
            }),
            row("Wide", theme, &|size, id| {
                Button::new(SharedString::from(format!("{id}-wide")), "Done")
                    .primary()
                    .wide()
                    .with_size(size)
                    .into_any_element()
            }),
            row("Cut short", theme, &|size, id| {
                Button::new(
                    SharedString::from(format!("{id}-cut")),
                    "A label too long for its width",
                )
                .w(FILL_WIDTH / 2.)
                .with_size(size)
                .into_any_element()
            }),
        ]);
        section("Button", theme, rows)
    }

    fn icon_buttons(theme: &Theme) -> AnyElement {
        let mut rows = vec![];
        for (state, selected, disabled) in [
            ("", false, false),
            ("selected", true, false),
            ("disabled", false, true),
        ] {
            let label = if state.is_empty() {
                "Icon".to_owned()
            } else {
                format!("Icon, {state}")
            };
            rows.push(row(&label, theme, &|size, id| {
                Button::icon(SharedString::from(format!("{id}-icon-{state}")), "settings")
                    .with_size(size)
                    .selected(selected)
                    .disabled(disabled)
                    .into_any_element()
            }));
        }
        rows.push(over_video(row("Over video", theme, &|size, id| {
            div()
                .flex()
                .gap_1()
                .child(
                    Button::new(SharedString::from(format!("{id}-overlay-ghost")), "Ghost")
                        .ghost()
                        .overlay()
                        .with_size(size),
                )
                .child(
                    Button::icon(SharedString::from(format!("{id}-overlay-icon")), "play")
                        .overlay()
                        .with_size(size),
                )
                .child(
                    Button::icon(SharedString::from(format!("{id}-overlay-selected")), "loop")
                        .overlay()
                        .selected(true)
                        .with_size(size),
                )
                .into_any_element()
        })));
        section("Icon button", theme, rows)
    }

    fn badges(theme: &Theme) -> AnyElement {
        let color: Hsla = rgb(BADGE_COLOR).into();
        section(
            "Badge",
            theme,
            vec![
                row("Label", theme, &|size, _| {
                    Badge::label("LIVE", color)
                        .with_size(size)
                        .into_any_element()
                }),
                row("Icon", theme, &|size, _| {
                    Badge::icon("cast", color)
                        .with_size(size)
                        .into_any_element()
                }),
                row("Accent", theme, &|size, _| {
                    Badge::label("HDR", theme.accent)
                        .with_size(size)
                        .into_any_element()
                }),
            ],
        )
    }

    fn icons(theme: &Theme) -> AnyElement {
        section(
            "Icon",
            theme,
            vec![
                row("Text color", theme, &|size, _| {
                    Icon::new("music").with_size(size).into_any_element()
                }),
                row("Muted", theme, &|size, _| {
                    Icon::new("music")
                        .color(theme.text_muted)
                        .with_size(size)
                        .into_any_element()
                }),
                row("Accent", theme, &|size, _| {
                    Icon::new("music")
                        .color(theme.accent)
                        .with_size(size)
                        .into_any_element()
                }),
            ],
        )
    }

    fn controls(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let on = self.on;
        let segment = self.segment;
        let value = self.value;
        let toggled = Rc::new(cx.listener(|this, on: &bool, _, cx| {
            this.on = *on;
            cx.notify();
        }));
        let picked = Rc::new(cx.listener(|this, segment: &usize, _, cx| {
            this.segment = *segment;
            cx.notify();
        }));
        let slid = Rc::new(cx.listener(|this, event: &SliderEvent, _, cx| {
            this.value = event.value;
            cx.notify();
        }));
        let dragging = &self.dragging;
        let slider = |size, id: &str, notched: bool, overlay: bool| {
            let slid = slid.clone();
            div()
                .w(FILL_WIDTH)
                .child(
                    Slider::new(
                        SharedString::from(format!("{id}-slider")),
                        value,
                        dragging.clone(),
                    )
                    .when(notched, |slider| slider.notches(vec![0.25, 0.5, 0.75]))
                    .when(overlay, Slider::overlay)
                    .with_size(size)
                    .on_change(move |event, window, cx| slid(event, window, cx)),
                )
                .into_any_element()
        };
        section(
            "Controls",
            theme,
            vec![
                row("Switch", theme, &|size, id| {
                    let toggled = toggled.clone();
                    Switch::new(SharedString::from(format!("{id}-switch")), on)
                        .with_size(size)
                        .on_toggle(move |on, window, cx| toggled(on, window, cx))
                        .into_any_element()
                }),
                row("Switch, disabled", theme, &|size, id| {
                    Switch::new(SharedString::from(format!("{id}-switch-disabled")), on)
                        .disabled(true)
                        .with_size(size)
                        .into_any_element()
                }),
                row("Segmented", theme, &|size, id| {
                    let picked = picked.clone();
                    Segmented::new(
                        SharedString::from(format!("{id}-segmented")),
                        &SEGMENTS,
                        segment,
                    )
                    .with_size(size)
                    .on_select(move |segment, window, cx| picked(segment, window, cx))
                    .into_any_element()
                }),
                row("Segmented, disabled", theme, &|size, id| {
                    Segmented::new(
                        SharedString::from(format!("{id}-segmented-disabled")),
                        &SEGMENTS,
                        segment,
                    )
                    .disabled(true)
                    .with_size(size)
                    .into_any_element()
                }),
                row("Slider", theme, &|size, id| slider(size, id, false, false)),
                row("Slider, notches", theme, &|size, id| {
                    slider(size, id, true, false)
                }),
                over_video(row("Slider, over video", theme, &|size, id| {
                    slider(size, id, false, true)
                })),
                row("Meter", theme, &|size, _| {
                    Meter::new(value)
                        .w(FILL_WIDTH)
                        .with_size(size)
                        .into_any_element()
                }),
                row("Text field", theme, &|size, _| {
                    let input = SIZES
                        .iter()
                        .position(|(each, _)| *each == size)
                        .map(|ix| self.inputs[ix].clone());
                    div().w(FILL_WIDTH).children(input).into_any_element()
                }),
            ],
        )
    }

    fn surfaces(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let checked = self.checked;
        let check = Rc::new(cx.listener(|this, row: &usize, _, cx| {
            this.checked = *row;
            cx.notify();
        }));
        let menu = |size: ComponentSize, id: &str, overlay: bool| {
            let rows = ["Auto", "English", "Off"]
                .into_iter()
                .enumerate()
                .map(|(ix, label)| {
                    let check = check.clone();
                    MenuRow::new(SharedString::from(format!("{id}-row-{ix}")), label)
                        .checked(ix == checked)
                        .when(ix == 1, |row| row.detail("AAC"))
                        .when(overlay, MenuRow::overlay)
                        .with_size(size)
                        .on_click(move |_, window, cx| check(&ix, window, cx))
                });
            let panel = Panel::new()
                .w(FILL_WIDTH)
                .py_1()
                .with_size(size)
                .children(rows);
            if overlay { panel.overlay() } else { panel }.into_any_element()
        };
        section(
            "Surfaces",
            theme,
            vec![
                row("Menu", theme, &|size, id| menu(size, id, false)),
                over_video(row("Menu, over video", theme, &|size, id| {
                    menu(size, id, true)
                })),
                over_video(row("Note", theme, &|size, _| {
                    Panel::note("Speed 1.5×").with_size(size).into_any_element()
                })),
                row("Card", theme, &|size, _| {
                    Card::new()
                        .w(FILL_WIDTH)
                        .with_size(size)
                        .child(div().p_3().child("In a card"))
                        .into_any_element()
                }),
            ],
        )
    }
}

impl Render for GalleryView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let sections = [
            Self::buttons(&theme),
            Self::icon_buttons(&theme),
            Self::badges(&theme),
            Self::icons(&theme),
            self.controls(&theme, cx),
            self.surfaces(&theme, cx),
        ];
        div()
            .id("gallery")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_6()
            .px_6()
            .pt_10()
            .pb_6()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .child(heading_row(&theme))
            .children(sections)
    }
}

/// The sizes' names over their columns.
fn heading_row(theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .text_label()
        .text_color(theme.text_muted)
        .child(div().w(LABEL_WIDTH).flex_none())
        .children(SIZES.map(|(_, name)| div().w(SIZE_WIDTH).flex_none().child(name)))
}

/// A heading with its rows under it.
fn section(title: &str, theme: &Theme, rows: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_heading()
                .pb_1()
                .border_b_1()
                .border_color(theme.border)
                .child(title.to_owned()),
        )
        .children(rows)
        .into_any_element()
}

/// `label`, then what `cell` draws in each size.
fn row(label: &str, theme: &Theme, cell: Draw) -> AnyElement {
    let id = label.to_lowercase().replace(", ", "-").replace(' ', "-");
    div()
        .flex()
        .items_center()
        .min_h_8()
        .child(
            div()
                .w(LABEL_WIDTH)
                .flex_none()
                .text_sm()
                .text_color(theme.text_muted)
                .child(label.to_owned()),
        )
        .children(SIZES.map(|(size, name)| {
            div()
                .w(SIZE_WIDTH)
                .flex_none()
                .flex()
                .items_center()
                .child(cell(size, &format!("{id}-{}", name.to_lowercase())))
        }))
        .into_any_element()
}

/// `row` on black, as over the picture, which is dark in both themes.
fn over_video(row: AnyElement) -> AnyElement {
    div()
        .bg(black())
        .rounded_md()
        .p_2()
        .child(row)
        .into_any_element()
}

#[cfg(test)]
#[path = "tests/gallery.rs"]
mod tests;
