//! About: the app's version and license; where updating is, with what
//! can be done about it, and once an update's found, what's new in it;
//! and what the app is built with: FFmpeg, dav1d, GPUI and Lua, from
//! `resources/attributions.json` (which `script/attributions` writes), each
//! opening to its license.

use std::ffi::CStr;
use std::sync::LazyLock;

use ffmpeg_next as ffmpeg;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, Window, div};
use serde::Deserialize;

use super::form::{group, row, set};
use super::{Pane, SettingsView};
use crate::app::updates::{self, Found, State};
use crate::settings::Settings;
use crate::ui::about;
use crate::ui::components::{Button, ComponentSize, Icon, Sizable, Switch};
use crate::ui::markdown;
use crate::ui::sentence::capitalized;
use crate::ui::theme::settings::{ABOUT_COLOR, CREDIT_LICENSE_MAX_WIDTH};
use crate::ui::theme::text::TextStyle;
use crate::ui::theme::{Theme, controls, radius};

const LICENSE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/LICENSE"));

/// The pane's name, which Check for Updates opens Settings at.
pub const NAME: &str = "About";

/// What `script/attributions` writes.
#[derive(Deserialize)]
struct Attributions {
    /// FFmpeg, dav1d, GPUI and Lua.
    libraries: Vec<Credit>,
    /// The license texts, which credits refer to by index.
    texts: Vec<String>,
}

#[derive(Deserialize)]
struct Credit {
    name: String,
    version: String,
    license: String,
    url: String,
    /// Where its source code is.
    source: String,
    texts: Vec<usize>,
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
        Self {
            key: key.into(),
            name: credit.name.as_str().into(),
            version: credit.version.as_str().into(),
            license: credit.license.as_str().into(),
            hint: None,
            links: vec![
                ("Website", credit.url.as_str().into()),
                ("Source code", credit.source.as_str().into()),
            ],
            note: None,
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

/// The app's own license.
fn license() -> Entry {
    Entry {
        key: "plays".into(),
        name: "License".into(),
        version: "".into(),
        license: env!("CARGO_PKG_LICENSE").into(),
        hint: None,
        links: Vec::new(),
        note: None,
        texts: vec![LICENSE.into()],
    }
}

/// What the app is built with, each said what it's for.
fn built_with() -> Vec<Entry> {
    let attributions = &*ATTRIBUTIONS;
    let texts = attributions.texts.as_slice();
    attributions
        .libraries
        .iter()
        .map(|library| credit(library, texts))
        .collect()
}

/// `library`'s credit, said what it's for.
fn credit(library: &'static Credit, texts: &'static [String]) -> Entry {
    let entry = Entry::new(format!("library {}", library.name), library, texts);
    match library.name.as_str() {
        "FFmpeg" => Entry {
            // What the app runs with, in case it was built with another.
            version: ffmpeg_version(),
            license: ffmpeg::util::license().into(),
            note: Some(
                "Plays is built with FFmpeg's source code, unchanged, by \
                 script/build-ffmpeg in Plays' own; build Plays again to use \
                 another FFmpeg."
                    .into(),
            ),
            ..entry.hint("Opens and decodes files and streams.")
        },
        "dav1d" => entry.hint("Decodes AV1 video, in FFmpeg."),
        "GPUI" => entry.hint("Draws the windows. Made by the Zed team at Zed Industries."),
        "Lua" => entry.hint("Runs the scripts plays do runs."),
        _ => entry,
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

/// Where updating is, as its row says it: what's happening, and more on
/// it.
fn said(state: &State) -> (String, Option<String>) {
    let version = updates::version();
    match state {
        State::Off(why) => (
            "Plays doesn't update itself here".into(),
            Some(capitalized(why)),
        ),
        State::Idle => (
            format!("Plays {version}"),
            Some("Not checked for updates yet.".into()),
        ),
        State::Checking => ("Checking for updates…".into(), None),
        State::UpToDate => (format!("Plays {version} is up to date"), None),
        State::Downloading { found, done } => (
            format!("Downloading Plays {}…", found.version),
            Some(format!("{} of {}", megabytes(*done), megabytes(found.size))),
        ),
        State::Ready(found) => (
            format!("Plays {} is ready to install", found.version),
            Some("It's installed as Plays restarts, or when it quits.".into()),
        ),
        State::Installed(found) => (
            format!("Plays {} is installed", found.version),
            Some("It runs once Plays starts again.".into()),
        ),
        State::Failed(why) => ("Plays couldn't update".into(), Some(capitalized(why))),
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1e6)
}

/// Where updating is, with the button for what can be done next.
fn status_row(state: &State, theme: &Theme) -> AnyElement {
    let (label, hint) = said(state);
    let control = match state {
        State::Off(_) => div().into_any_element(),
        State::Ready(_) | State::Installed(_) => {
            Button::new("updates-restart", "Restart to Update")
                .on_click(|_, _, cx| {
                    if let Err(why) = updates::restart(cx) {
                        log::warn!("{why}");
                    }
                })
                .into_any_element()
        }
        _ => Button::new("updates-check", "Check Now")
            .disabled(matches!(state, State::Checking | State::Downloading { .. }))
            .on_click(|_, _, cx| updates::check(cx))
            .into_any_element(),
    };
    row(label, hint.map(SharedString::from), theme, control)
}

/// Whether Plays checks by itself.
fn automatic_row(s: &Settings, state: &State, theme: &Theme) -> AnyElement {
    row(
        "Check for updates automatically",
        Some(
            "A minute after Plays starts, then once a day. An update found downloads by \
             itself."
                .into(),
        ),
        theme,
        Switch::new("check-for-updates", s.check_for_updates)
            .disabled(matches!(state, State::Off(_)))
            .on_toggle(|&on, _, cx| set("check_for_updates", on, cx)),
    )
}

/// What's new since this version: each release's notes, newest first.
fn whats_new(found: &Found, theme: &Theme) -> AnyElement {
    div()
        .debug_selector(|| "release-notes".into())
        .flex()
        .flex_col()
        .gap_4()
        .px_3()
        .py_3()
        .children(found.releases.iter().map(|release| {
            let version = release.version.to_string();
            let blocks = markdown::parse(&release.notes);
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_muted)
                        .child(format!("Plays {version}")),
                )
                .child(markdown::render(
                    &blocks,
                    &format!("notes-{version}"),
                    theme,
                ))
        }))
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
        NAME
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
        s: &Settings,
        theme: &Theme,
        _: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        let state = updates::state(cx);
        let open = view.license.clone();
        let mut credit_row =
            |entry: &Entry| entry.render(open.as_ref() == Some(&entry.key), theme, cx);
        let license = credit_row(&license());
        let built_with = built_with().iter().map(&mut credit_row).collect();
        let mut groups = vec![
            group(None, theme, vec![app(theme), license]),
            group(
                Some("Updates"),
                theme,
                vec![status_row(&state, theme), automatic_row(s, &state, theme)],
            ),
        ];
        if let Some(found) = state.found() {
            groups.push(group(
                Some("What's new"),
                theme,
                vec![whats_new(found, theme)],
            ));
        }
        groups.push(group(Some("Built with"), theme, built_with));
        groups
    }

    fn leave(&self, view: &mut SettingsView) {
        view.license = None;
    }
}

#[cfg(test)]
#[path = "tests/about.rs"]
mod tests;
