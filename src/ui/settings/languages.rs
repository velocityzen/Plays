//! The languages Audio and Subtitles prefer, first first: a file opens
//! with its sound, or its subtitles, in the first of them it has. Each
//! moves up or down or goes, and Add Language puts another last.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Corner, SharedString, div, point};

use super::SettingsView;
use super::form::{group, row, set};
use crate::media::tracks::{language_name, languages};
use crate::settings::Settings;
use crate::ui::components::{Button, MenuRow, Panel, Popover, Sizable as _};
use crate::ui::theme::Theme;
use crate::ui::theme::settings::{
    LANGUAGE_BUTTON_WIDTH, LANGUAGE_MENU_MAX_HEIGHT, LANGUAGE_MENU_TOP, LANGUAGE_MENU_WIDTH,
};

/// Which list of languages: one pane's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum List {
    Audio,
    Subtitles,
}

impl List {
    /// The setting it is.
    fn key(self) -> &'static str {
        match self {
            Self::Audio => "languages.audio",
            Self::Subtitles => "languages.subtitles",
        }
    }

    /// What its buttons and menu are tagged with.
    fn id(self) -> &'static str {
        match self {
            Self::Audio => "audio-languages",
            Self::Subtitles => "subtitle-languages",
        }
    }

    /// The languages it has, first first.
    fn of(self, settings: &Settings) -> &[String] {
        match self {
            Self::Audio => &settings.languages.audio,
            Self::Subtitles => &settings.languages.subtitles,
        }
    }

    /// What the first row says, and its hint.
    fn says(self) -> (&'static str, &'static str) {
        match self {
            Self::Audio => (
                "Play audio in",
                "The first of these a file has, else its own. Audio picked in a file plays \
                 again as it opens again, while files resume where they were left.",
            ),
            Self::Subtitles => (
                "Show subtitles in",
                "The first of these a file has, else those beside it or its own. \
                 Subtitles picked or hidden in a file are again as it opens again, while \
                 files resume where they were left.",
            ),
        }
    }
}

/// The group of `list`'s languages: what it does, with Add Language, then
/// each language, first first.
pub(super) fn languages_group(
    view: &SettingsView,
    list: List,
    s: &Settings,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let chosen = list.of(s);
    let (label, hint) = list.says();
    let mut rows = vec![row(
        label,
        Some(hint.into()),
        theme,
        add_language(view, list, chosen, theme, cx),
    )];
    rows.extend(
        chosen
            .iter()
            .enumerate()
            .map(|(at, code)| row(language_name(code), None, theme, moves(list, chosen, at))),
    );
    group(Some("Languages"), theme, rows)
}

/// `chosen` with the language at `at` moved to `to`.
fn moved(chosen: &[String], at: usize, to: usize) -> Vec<String> {
    let mut languages = chosen.to_vec();
    let language = languages.remove(at);
    languages.insert(to, language);
    languages
}

/// The buttons that move the language at `at` up or down, or take it off.
fn moves(list: List, chosen: &[String], at: usize) -> impl IntoElement {
    let key = list.key();
    let id = |what: &str| (SharedString::from(format!("{}-{what}", list.id())), at);
    let last = chosen.len() - 1;
    let (up, down, off) = (
        moved(chosen, at, at.saturating_sub(1)),
        moved(chosen, at, (at + 1).min(last)),
        chosen
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != at)
            .map(|(_, code)| code.clone())
            .collect::<Vec<_>>(),
    );
    div()
        .flex()
        .flex_none()
        .gap_1()
        .child(
            Button::icon(id("up"), "chevron-up")
                .small()
                .disabled(at == 0)
                .on_click(move |_, _, cx| set(key, &up, cx)),
        )
        .child(
            Button::icon(id("down"), "chevron-down")
                .small()
                .disabled(at == last)
                .on_click(move |_, _, cx| set(key, &down, cx)),
        )
        .child(
            Button::icon(id("remove"), "close")
                .small()
                .on_click(move |_, _, cx| set(key, &off, cx)),
        )
}

/// The button that opens the menu of the languages not in `list`.
fn add_language(
    view: &SettingsView,
    list: List,
    chosen: &[String],
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let id = list.id();
    Popover::new(
        SharedString::from(format!("{id}-backdrop")),
        Button::new(SharedString::from(format!("{id}-add")), "Add Language")
            .icon_left("plus")
            .w(LANGUAGE_BUTTON_WIDTH)
            .on_click(cx.listener(move |view, _, _, cx| {
                view.language_menu = Some(list);
                cx.notify();
            })),
    )
    .anchor(
        Corner::TopRight,
        point(LANGUAGE_BUTTON_WIDTH, LANGUAGE_MENU_TOP),
    )
    .when(view.language_menu == Some(list), |this| {
        this.menu(language_menu(list, chosen, theme, cx))
    })
    .on_dismiss(cx.listener(|view, _, _, cx| {
        view.language_menu = None;
        cx.notify();
    }))
    .into_any_element()
}

/// Every language not in `chosen`, A to Z, each adding itself last.
fn language_menu(
    list: List,
    chosen: &[String],
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> impl IntoElement {
    let rows = languages()
        .into_iter()
        .filter(|(code, _)| !chosen.iter().any(|c| c == code))
        .map(|(code, name)| {
            let mut added = chosen.to_vec();
            added.push(code.to_owned());
            MenuRow::new(SharedString::from(format!("{}-{code}", list.id())), name)
                .detail(div().text_color(theme.text_muted).child(code))
                .mx_1()
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.language_menu = None;
                    set(list.key(), &added, cx);
                    cx.notify();
                }))
        })
        .collect::<Vec<_>>();
    Panel::new().occlude().w(LANGUAGE_MENU_WIDTH).child(
        div()
            .id(SharedString::from(format!("{}-menu", list.id())))
            .max_h(LANGUAGE_MENU_MAX_HEIGHT)
            .overflow_y_scroll()
            .py_1()
            .children(rows),
    )
}

#[cfg(test)]
#[path = "tests/languages.rs"]
mod tests;
