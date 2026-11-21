//! About: the app's version and license, and what it is built with, each
//! with its license, from `resources/attributions.json` (which
//! `script/attributions` writes). There are hundreds, so they are in a
//! list that builds only the rows in view.

#[cfg(test)]
use std::cell::Cell;
use std::ffi::CStr;
use std::sync::LazyLock;

use ffmpeg_next as ffmpeg;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, ListAlignment, ListState, SharedString, Window, div, list};
use serde::Deserialize;

use super::form::{card_row, group, heading};
use super::{Pane, SettingsView};
use crate::settings::Settings;
use crate::ui::about;
use crate::ui::components::{Button, ComponentSize, Icon, Sizable};
use crate::ui::theme::settings::{ABOUT_COLOR, CREDIT_LICENSE_MAX_WIDTH, CREDITS_OVERDRAW};
use crate::ui::theme::text::TextStyle;
use crate::ui::theme::{Theme, controls, radius};

const LICENSE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/LICENSE"));

/// What `script/attributions` writes.
#[derive(Deserialize)]
struct Attributions {
    /// FFmpeg and dav1d.
    libraries: Vec<Credit>,
    crates: Vec<Credit>,
    /// The license texts, which credits refer to by index.
    texts: Vec<String>,
}

#[derive(Deserialize)]
struct Credit {
    name: String,
    version: String,
    license: String,
    url: String,
    /// Where its source code can be downloaded, for FFmpeg and dav1d.
    #[serde(default)]
    source: Option<String>,
    /// Where a crate is built in: `macos`, `linux` or both.
    #[serde(default)]
    platforms: Vec<String>,
    /// Who holds the copyright of a crate that comes with no license file,
    /// whose texts are its license's standard ones.
    #[serde(default)]
    authors: Option<Vec<String>>,
    texts: Vec<usize>,
}

impl Credit {
    /// Whether it's in the app on this platform.
    fn built_in(&self) -> bool {
        let platform = if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        self.platforms.is_empty() || self.platforms.iter().any(|p| p == platform)
    }
}

#[cfg(test)]
thread_local! {
    /// How many credits' rows have been built, for the tests to see.
    pub(super) static BUILT: Cell<usize> = const { Cell::new(0) };
}

/// Read when About first shows.
static ATTRIBUTIONS: LazyLock<Attributions> = LazyLock::new(|| {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/resources/attributions.json"
    )))
    .expect("resources/attributions.json is as script/attributions writes it")
});

/// A credit's row: its name and version, what it is for if that's said, and
/// its license, whose text shows below it, with its links, once clicked.
struct Entry {
    /// Which one shows its license (`SettingsView::license`).
    key: SharedString,
    name: SharedString,
    version: SharedString,
    license: SharedString,
    hint: Option<SharedString>,
    links: Vec<(&'static str, SharedString)>,
    /// Said above the license texts.
    note: Option<SharedString>,
    texts: Vec<SharedString>,
}

impl Entry {
    fn new(
        key: impl Into<SharedString>,
        credit: &'static Credit,
        texts: &'static [String],
    ) -> Self {
        let mut links = vec![("Website", credit.url.as_str().into())];
        links.extend(
            credit
                .source
                .as_deref()
                .map(|source| ("Source code", source.into())),
        );
        Self {
            key: key.into(),
            name: credit.name.as_str().into(),
            version: credit.version.as_str().into(),
            license: credit.license.as_str().into(),
            hint: None,
            links,
            note: credit.authors.as_ref().map(|authors| {
                let holders = if authors.is_empty() {
                    "its authors".to_owned()
                } else {
                    authors.join(", ")
                };
                format!(
                    "It comes without a license file, so this is its license's \
                     standard text. Copyright {holders}."
                )
                .into()
            }),
            texts: credit
                .texts
                .iter()
                .map(|&index| texts[index].as_str().into())
                .collect(),
        }
    }

    fn hint(mut self, hint: &'static str) -> Self {
        self.hint = Some(hint.into());
        self
    }

    fn render(&self, open: bool, theme: &Theme, cx: &mut Context<SettingsView>) -> AnyElement {
        #[cfg(test)]
        BUILT.with(|built| built.set(built.get() + 1));
        let heading = self.render_heading(open, theme, cx);
        let details = self.render_details(theme);
        div()
            .flex()
            .flex_col()
            .child(heading)
            .when(open, |this| this.child(details))
            .into_any_element()
    }

    /// The row that opens and closes it: its name and version, a hint, and
    /// its license's name.
    fn render_heading(
        &self,
        open: bool,
        theme: &Theme,
        cx: &mut Context<SettingsView>,
    ) -> impl IntoElement {
        let muted = theme.text_muted;
        let key = self.key.clone();
        let toggle = cx.listener(move |view, _, _, cx| {
            let shown = view.license.take();
            if shown.as_ref() != Some(&key) {
                view.license = Some(key.clone());
            }
            // The rows it opens and closes in are measured again.
            if let Some(credits) = &view.credits {
                for key in shown.iter().chain(&view.license) {
                    if let Some(item) = credits.item_of(key) {
                        credits.list.splice(item..item + 1, 1);
                    }
                }
            }
            cx.notify();
        });
        let chevron = Icon::new(if open {
            "chevron-down"
        } else {
            "chevron-right"
        })
        .small()
        .color(muted);
        div()
            .debug_selector({
                let key = self.key.clone();
                move || format!("credit-{key}")
            })
            .id(self.key.clone())
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .cursor_pointer()
            .on_click(toggle)
            .child(chevron)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap_1p5()
                            .text_sm()
                            .child(self.name.clone())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(self.version.clone()),
                            ),
                    )
                    .when_some(self.hint.clone(), |this, hint| {
                        this.child(div().text_xs().text_color(muted).child(hint))
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .max_w(CREDIT_LICENSE_MAX_WIDTH)
                    .truncate()
                    .text_xs()
                    .text_color(muted)
                    .child(self.license.clone()),
            )
    }

    /// What shows while it is open: its links, any note on its license, and
    /// the license texts.
    fn render_details(&self, theme: &Theme) -> Div {
        let muted = theme.text_muted;
        div()
            .debug_selector(|| "credit-texts".into())
            .flex()
            .gap_2()
            .px_3()
            .pb_3()
            // Under the name, past the chevron.
            .child(
                div()
                    .flex_none()
                    .w(ComponentSize::Small.pick(controls::ICON)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .gap_4()
                            .children(self.links.iter().cloned().map(|(label, url)| {
                                Button::new(label, label)
                                    .link()
                                    .small()
                                    .on_click(move |_, _, cx| cx.open_url(&url))
                            })),
                    )
                    .when_some(self.note.clone(), |this, note| {
                        this.child(div().text_xs().text_color(muted).child(note))
                    })
                    .children(self.texts.iter().cloned().map(|text| {
                        div()
                            .px_3()
                            .py_2()
                            .superellipse(ComponentSize::Medium.pick(radius::ITEM))
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.background)
                            .text_xs()
                            .text_color(muted)
                            .child(text)
                    })),
            )
    }
}

/// How many crates About credits on this platform.
#[cfg(test)]
pub(super) fn crates_built_in() -> usize {
    ATTRIBUTIONS
        .crates
        .iter()
        .filter(|credit| credit.built_in())
        .count()
}

/// About's rows, made when it first shows and kept while the window is
/// open.
pub(super) struct Credits {
    /// The app and its license, what it is built with, the crates' title,
    /// then each crate.
    list: ListState,
    license: Entry,
    built_with: Vec<Entry>,
    crates: Vec<Entry>,
}

/// The list's items before the crates.
const BEFORE_CRATES: usize = 3;

impl Credits {
    fn new() -> Self {
        let attributions = &*ATTRIBUTIONS;
        let texts = attributions.texts.as_slice();
        let license = Entry {
            key: "plays".into(),
            name: "License".into(),
            version: "".into(),
            license: env!("CARGO_PKG_LICENSE").into(),
            hint: None,
            links: Vec::new(),
            note: None,
            texts: vec![LICENSE.into()],
        };

        let mut built_with = Vec::new();
        for library in &attributions.libraries {
            let key = format!("library {}", library.name);
            let mut entry = Entry::new(key, library, texts);
            if library.name == "FFmpeg" {
                // What the app runs with, in case it was built with another.
                entry.version = ffmpeg_version();
                entry.license = ffmpeg::util::license().into();
                entry.note = Some(
                    "Plays is built with FFmpeg's source code, unchanged, by \
                     script/build-ffmpeg in Plays' own; build Plays again to use \
                     another FFmpeg."
                        .into(),
                );
                entry = entry.hint("Opens and decodes files and streams.");
            } else {
                entry = entry.hint("Decodes AV1 video, in FFmpeg.");
            }
            built_with.push(entry);
        }
        if let Some(gpui) = attributions.crates.iter().find(|c| c.name == "gpui") {
            built_with.push(
                Entry::new("library gpui", gpui, texts)
                    .hint("Draws the windows, from Zed Industries."),
            );
        }

        let crates: Vec<Entry> = attributions
            .crates
            .iter()
            .filter(|credit| credit.built_in())
            .map(|credit| {
                let key = format!("crate {} {}", credit.name, credit.version);
                let mut entry = Entry::new(key, credit, texts);
                entry.links[0].0 = "Repository";
                entry
            })
            .collect();
        Self {
            list: ListState::new(
                BEFORE_CRATES + crates.len(),
                ListAlignment::Top,
                CREDITS_OVERDRAW,
            ),
            license,
            built_with,
            crates,
        }
    }

    /// The list item the credit `key` is in.
    fn item_of(&self, key: &SharedString) -> Option<usize> {
        if self.license.key == *key {
            Some(0)
        } else if self.built_with.iter().any(|entry| entry.key == *key) {
            Some(1)
        } else {
            let index = self.crates.iter().position(|entry| entry.key == *key)?;
            Some(BEFORE_CRATES + index)
        }
    }
}

/// The app's name, version and what it is.
fn app(theme: &Theme) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_3()
        .child(div().text_display().child("Plays"))
        .child(div().text_sm().text_color(theme.text_muted).child(format!(
            "{}, {}",
            about::version(),
            about::release_date()
        )))
        .child(div().pt_1().text_sm().child(env!("CARGO_PKG_DESCRIPTION")))
        .into_any_element()
}

/// About's list item `ix`, spaced as other panes' groups are.
fn item(
    view: &mut SettingsView,
    ix: usize,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let Some(credits) = view.credits.as_ref() else {
        return div().into_any_element();
    };
    let theme = Theme::get(window, cx);
    let open = view.license.as_ref();
    let row = |entry: &Entry, cx: &mut Context<SettingsView>| {
        entry.render(open == Some(&entry.key), &theme, cx)
    };
    let (content, below) = match ix {
        0 => (
            group(None, &theme, vec![app(&theme), row(&credits.license, cx)]),
            true,
        ),
        1 => (
            group(
                Some("Built with"),
                &theme,
                credits
                    .built_with
                    .iter()
                    .map(|entry| row(entry, cx))
                    .collect(),
            ),
            true,
        ),
        2 => (heading("Rust crates"), false),
        _ => {
            let index = ix - BEFORE_CRATES;
            let Some(entry) = credits.crates.get(index) else {
                return div().into_any_element();
            };
            let last = index + 1 == credits.crates.len();
            (card_row(&theme, index == 0, last, row(entry, cx)), false)
        }
    };
    div()
        .px_5()
        .map(|this| match (ix, below) {
            (_, true) => this.pb_5(),
            (2, _) => this.pb_2(),
            _ => this,
        })
        .child(content)
        .into_any_element()
}

/// The version of the FFmpeg the app runs with, which is built into it.
pub fn ffmpeg_version() -> SharedString {
    // SAFETY: av_version_info returns a static, NUL-terminated string.
    let version = unsafe { CStr::from_ptr(ffmpeg::ffi::av_version_info()) };
    version.to_string_lossy().into_owned().into()
}

pub(super) struct AboutPane;

impl Pane for AboutPane {
    fn name(&self) -> &'static str {
        "About"
    }

    fn icon(&self) -> &'static str {
        "info"
    }

    fn color(&self) -> u32 {
        ABOUT_COLOR
    }

    /// Its rows are in its list instead.
    fn content(
        &self,
        _: &mut SettingsView,
        _: &Settings,
        _: &Theme,
        _: &mut Window,
        _: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        Vec::new()
    }

    fn list(&self, view: &mut SettingsView, cx: &mut Context<SettingsView>) -> Option<AnyElement> {
        let credits = view.credits.get_or_insert_with(Credits::new);
        Some(
            list(credits.list.clone(), cx.processor(item))
                .size_full()
                .pt_2()
                .pb_5()
                .into_any_element(),
        )
    }

    fn leave(&self, view: &mut SettingsView) {
        view.license = None;
        // Measured again, and at the top, when it shows again.
        if let Some(credits) = &view.credits {
            credits.list.reset(credits.list.item_count());
        }
    }
}
