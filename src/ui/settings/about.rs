//! About: the app's version and license, and what it is built with, each
//! with its license, from `resources/attributions.json` (which
//! `script/attributions` writes).

use std::ffi::CStr;
use std::sync::LazyLock;

use ffmpeg_next as ffmpeg;
use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, Window, div};
use serde::Deserialize;

use super::form::group;
use super::{Pane, SettingsView};
use crate::settings::Settings;
use crate::ui::components::{Button, ComponentSize, Icon, Sizable};
use crate::ui::theme::settings::{ABOUT_COLOR, CREDIT_LICENSE_MAX_WIDTH};
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

    fn render(self, open: bool, theme: &Theme, cx: &mut Context<SettingsView>) -> AnyElement {
        let muted = theme.text_muted;
        let key = self.key.clone();
        let toggle = cx.listener(move |view, _, _, cx| {
            view.license = if view.license.as_ref() == Some(&key) {
                None
            } else {
                Some(key.clone())
            };
            cx.notify();
        });
        let chevron = Icon::new(if open {
            "chevron-down"
        } else {
            "chevron-right"
        })
        .small()
        .color(muted);
        let details = div()
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
                    .child(div().flex().gap_4().children(self.links.into_iter().map(
                        |(label, url)| {
                            Button::new(label, label)
                                .link()
                                .small()
                                .on_click(move |_, _, cx| cx.open_url(&url))
                        },
                    )))
                    .when_some(self.note, |this, note| {
                        this.child(div().text_xs().text_color(muted).child(note))
                    })
                    .children(self.texts.into_iter().map(|text| {
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
            );
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(self.key)
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
                                    .child(self.name)
                                    .child(div().text_xs().text_color(muted).child(self.version)),
                            )
                            .when_some(self.hint, |this, hint| {
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
                            .child(self.license),
                    ),
            )
            .when(open, |this| this.child(details))
            .into_any_element()
    }
}

/// The version of the FFmpeg the app runs with, which is built into it.
fn ffmpeg_version() -> SharedString {
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

    fn content(
        &self,
        view: &mut SettingsView,
        _: &Settings,
        theme: &Theme,
        _: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let attributions = &*ATTRIBUTIONS;
        let texts = attributions.texts.as_slice();
        let open = view.license.clone();
        let mut row = |entry: Entry| {
            let shows = open.as_ref() == Some(&entry.key);
            entry.render(shows, theme, cx)
        };

        let app = div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_3()
            .child(div().text_display().child("Plays"))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.text_muted)
                    .child(concat!("Version ", env!("CARGO_PKG_VERSION"))),
            )
            .child(div().pt_1().text_sm().child(env!("CARGO_PKG_DESCRIPTION")))
            .into_any_element();
        let license = row(Entry {
            key: "plays".into(),
            name: "License".into(),
            version: "".into(),
            license: env!("CARGO_PKG_LICENSE").into(),
            hint: None,
            links: Vec::new(),
            note: None,
            texts: vec![LICENSE.into()],
        });

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
            built_with.push(row(entry));
        }
        if let Some(gpui) = attributions.crates.iter().find(|c| c.name == "gpui") {
            built_with.push(row(Entry::new("library gpui", gpui, texts)
                .hint("Draws the windows, from Zed Industries.")));
        }

        let crates = attributions
            .crates
            .iter()
            .filter(|credit| credit.built_in())
            .map(|credit| {
                let key = format!("crate {} {}", credit.name, credit.version);
                let mut entry = Entry::new(key, credit, texts);
                entry.links[0].0 = "Repository";
                row(entry)
            })
            .collect();

        vec![
            group(None, theme, vec![app, license]),
            group(Some("Built with"), theme, built_with),
            group(Some("Rust crates"), theme, crates),
        ]
    }

    fn leave(&self, view: &mut SettingsView) {
        view.license = None;
    }
}
