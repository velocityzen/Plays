//! What the panes are made of: cards of rows, each a label (and a hint)
//! with a control, and the sliders they share.

use std::ops::RangeInclusive;

use gpui::prelude::*;
use gpui::{AnyElement, App, SharedString, div};

use serde::Serialize;
use serde_json::json;

use super::SettingsView;
use crate::api::{self, Invocation};
use crate::settings::{Settings, fields};
use crate::ui::components::{Card, Slide, Slider};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::{SLIDER_VALUE_WIDTH, SLIDER_WIDTH};
use crate::ui::theme::text::TextStyle;

pub(super) fn update_settings(cx: &mut App, save: bool, change: impl FnOnce(&mut Settings)) {
    Settings::update(cx, save, change);
}

/// Set the setting `key` to `value` with `app.settings.set`, once the event is
/// handled, as every control does.
pub(super) fn set(key: impl Into<String>, value: impl Serialize, cx: &mut App) {
    let args = json!({"key": key.into(), "value": value});
    api::dispatch(Invocation::new("app.settings.set").with_args(args), cx);
}

/// Put the setting `key` back as Plays starts it out, with `app.settings.reset`.
pub(super) fn reset(key: &str, cx: &mut App) {
    api::dispatch(
        Invocation::new("app.settings.reset").with_args(json!({"key": key})),
        cx,
    );
}

/// What a slider over the setting `key` does as it moves to `value`: shows
/// it there while dragged, without saving, and sets it once let go.
pub(super) fn slide(key: &'static str, value: impl Serialize, slide: Slide, cx: &mut App) {
    if slide == Slide::End {
        set(key, value, cx);
        return;
    }
    let Ok(value) = serde_json::to_value(value) else {
        return;
    };
    Settings::update(cx, false, |settings| {
        if let Some(field) = fields::find(key, &[])
            && let Ok(moved) = fields::with(settings, &field, value, &[])
        {
            *settings = moved;
        }
    });
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
fn heading(title: &'static str) -> AnyElement {
    div().px_1().text_heading().child(title).into_any_element()
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

/// A slider over the setting `key`, from none to all, shown as a
/// percentage.
pub(super) fn percent_slider(
    view: &mut SettingsView,
    id: &'static str,
    key: &'static str,
    value: f32,
    theme: &Theme,
) -> AnyElement {
    range_slider(view, id, key, value, 0.0..=1.0, 100.0, theme)
}

/// A slider over the setting `key`, across `range`, shown as a
/// percentage, that moves in steps of `1 / steps` so it lands on round
/// numbers.
pub(super) fn range_slider(
    view: &mut SettingsView,
    id: &'static str,
    key: &'static str,
    value: f32,
    range: RangeInclusive<f32>,
    steps: f32,
    theme: &Theme,
) -> AnyElement {
    let (low, high) = (*range.start(), *range.end());
    let position = (value - low) / (high - low);
    labeled_slider(
        Slider::new(id, position, view.drag(id)).on_change(move |event, _, cx| {
            let value = ((low + event.value * (high - low)) * steps).round() / steps;
            slide(key, value, event.slide, cx);
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
