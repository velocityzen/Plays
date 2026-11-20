//! What the panes are made of: cards of rows, each a label (and a hint)
//! with a control, and the sliders they share.

use std::ops::RangeInclusive;

use gpui::prelude::*;
use gpui::{AnyElement, App, SharedString, div};

use super::SettingsView;
use crate::settings::Settings;
use crate::ui::components::{Card, ComponentSize, Slide, Slider};
use crate::ui::theme::settings::{SLIDER_VALUE_WIDTH, SLIDER_WIDTH};
use crate::ui::theme::text::TextStyle;
use crate::ui::theme::{Theme, radius};

pub(super) fn update_settings(cx: &mut App, save: bool, change: impl FnOnce(&mut Settings)) {
    Settings::update(cx, save, change);
}

/// Rows on one card, under an optional heading.
pub(super) fn group(
    title: Option<&'static str>,
    theme: &Theme,
    rows: Vec<AnyElement>,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .when_some(title, |this, title| this.child(heading(title)))
        .child(
            Card::new()
                .flex()
                .flex_col()
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

/// A group's title, above its card.
pub(super) fn heading(title: &'static str) -> AnyElement {
    div().px_1().text_heading().child(title).into_any_element()
}

/// One row of a group's card, drawn on its own for a list that builds
/// only the rows in view: the card's sides, its top over the first row
/// and its bottom under the last, and a line between rows.
pub(super) fn card_row(theme: &Theme, first: bool, last: bool, row: AnyElement) -> AnyElement {
    let radius = ComponentSize::default().pick(radius::CARD);
    div()
        .bg(theme.surface)
        .border_color(theme.border)
        .border_l_1()
        .border_r_1()
        .border_t_1()
        .when(last, |this| this.border_b_1())
        .when(first || last, |this| this.superellipse(radius))
        .when(first && !last, |this| this.rounded_b_none())
        .when(last && !first, |this| this.rounded_t_none())
        .child(row)
        .into_any_element()
}

pub(super) fn row(
    label: impl Into<SharedString>,
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
                .child(div().text_sm().child(label.into()))
                .when_some(hint, |this, hint| {
                    this.child(div().text_xs().text_color(theme.text_muted).child(hint))
                }),
        )
        .child(control)
        .into_any_element()
}

pub(super) fn percent_slider(
    view: &mut SettingsView,
    id: &'static str,
    value: f32,
    theme: &Theme,
    apply: impl Fn(&mut Settings, f32) + Copy + 'static,
) -> AnyElement {
    range_slider(view, id, value, 0.0..=1.0, 100.0, theme, apply)
}

/// A slider over `range`, shown as a percentage, that moves in steps of
/// `1 / steps` so it lands on round numbers.
pub(super) fn range_slider(
    view: &mut SettingsView,
    id: &'static str,
    value: f32,
    range: RangeInclusive<f32>,
    steps: f32,
    theme: &Theme,
    apply: impl Fn(&mut Settings, f32) + Copy + 'static,
) -> AnyElement {
    let (low, high) = (*range.start(), *range.end());
    let position = (value - low) / (high - low);
    labeled_slider(
        Slider::new(id, position, view.drag(id)).on_change(move |event, _, cx| {
            let value = ((low + event.value * (high - low)) * steps).round() / steps;
            update_settings(cx, event.slide == Slide::End, |s| apply(s, value));
        }),
        format!("{}%", (value * 100.0).round()),
        theme,
    )
}

/// A slider with its value shown next to it.
pub(super) fn labeled_slider(slider: Slider, value: String, theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .w(SLIDER_WIDTH)
        .child(slider)
        .child(
            div()
                .w(SLIDER_VALUE_WIDTH)
                .flex_none()
                .text_right()
                .text_sm()
                .text_color(theme.text_muted)
                .child(value),
        )
        .into_any_element()
}
