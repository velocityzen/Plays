//! Default Player: each format Plays opens, with the app that opens its
//! files now, and the formats picked for Plays to open from then on. macOS
//! asks to confirm each one, so only those picked are asked about.

use std::collections::BTreeSet;

use file_types::{Format, Kind};
use gpui::{AnyElement, Context, SharedString, Window, div};

use super::form::{group, row};
use super::{Pane, SettingsView};
use crate::app::default_player::{Opener, Openers, System};
use crate::settings::Settings;
use crate::ui::components::{Button, Switch};
use crate::ui::sentence::capitalized;
use crate::ui::theme::Theme;
use crate::ui::theme::settings::DEFAULT_PLAYER_COLOR;

/// The pane's name, which the command opens Settings at.
pub const NAME: &str = "Default Player";

pub(super) struct DefaultPlayerPane;

/// What the pane shows and has picked, which the Settings window keeps.
#[derive(Default)]
pub(super) struct State {
    /// Each format and the app that opens it, once read: again each time
    /// the pane shows, and after Plays is made the default.
    openers: Option<Openers>,
    reading: bool,
    /// The formats picked for Plays, by extension.
    picked: BTreeSet<&'static str>,
    /// Whether the system is being asked now.
    asking: bool,
    /// What asking came to last.
    made: Option<String>,
}

impl Pane for DefaultPlayerPane {
    fn name(&self) -> &'static str {
        NAME
    }

    fn icon(&self) -> &'static str {
        "file-play"
    }

    fn color(&self) -> u32 {
        DEFAULT_PLAYER_COLOR
    }

    fn content(
        &self,
        view: &mut SettingsView,
        _: &Settings,
        theme: &Theme,
        _: &mut Window,
        cx: &mut Context<SettingsView>,
    ) -> Vec<AnyElement> {
        if view.default_player.openers.is_none() && !view.default_player.reading {
            read(view, cx);
        }
        let cant = (System::get(cx).cant)();
        let state = &view.default_player;
        let mut groups = vec![group(
            None,
            theme,
            vec![ask_row(state, cant.as_ref(), theme, cx)],
        )];
        match &state.openers {
            None => groups.push(group(
                None,
                theme,
                vec![row("Looking…", None, theme, div())],
            )),
            Some(Err(why)) => groups.push(group(
                None,
                theme,
                vec![row(
                    "Plays can't tell which app opens each format",
                    Some(why.clone().into()),
                    theme,
                    div(),
                )],
            )),
            Some(Ok(openers)) => {
                let locked = cant.is_some() || state.asking;
                for (kind, title) in [(Kind::Video, "Video"), (Kind::Audio, "Audio")] {
                    let of_kind: Vec<&Opener> =
                        openers.iter().filter(|o| o.format.kind == kind).collect();
                    let rows = kind_rows(kind, &of_kind, &state.picked, locked, theme, cx);
                    groups.push(group(Some(title), theme, rows));
                }
            }
        }
        groups
    }

    fn leave(&self, view: &mut SettingsView) {
        let state = &mut view.default_player;
        state.openers = None;
        state.picked.clear();
        state.made = None;
    }
}

/// The row that asks for Plays to open the formats picked, and says what
/// asking came to, or why it can't.
fn ask_row(
    state: &State,
    cant: Option<&String>,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let count = state.picked.len();
    let hint = match (cant, state.asking, &state.made) {
        (Some(cant), _, _) => capitalized(cant),
        (None, true, _) if cfg!(target_os = "macos") => {
            "macOS asks about each format: Use to have Plays open it, Keep for the app it has"
                .to_owned()
        }
        (None, true, _) => "Setting…".to_owned(),
        (None, false, Some(made)) => made.clone(),
        (None, false, None) if cfg!(target_os = "macos") => {
            "Pick formats below for Plays to open. macOS asks you to confirm each one.".to_owned()
        }
        (None, false, None) => "Pick formats below for Plays to open.".to_owned(),
    };
    let label = match count {
        0 | 1 => "Use Plays".to_owned(),
        count => format!("Use Plays for {count}"),
    };
    row(
        "Open files with Plays",
        Some(SharedString::from(hint)),
        theme,
        Button::new("default-player-use", label)
            .disabled(count == 0 || cant.is_some() || state.asking)
            .on_click(cx.listener(|view, _, _, cx| ask(view, cx))),
    )
}

/// A kind's rows: one that picks all its formats, then one a format.
fn kind_rows(
    kind: Kind,
    openers: &[&Opener],
    picked: &BTreeSet<&'static str>,
    locked: bool,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> Vec<AnyElement> {
    let others: Vec<&'static str> = openers
        .iter()
        .filter(|opener| !opener.plays)
        .map(|opener| opener.format.extension())
        .collect();
    let all = !others.is_empty() && others.iter().all(|extension| picked.contains(extension));
    let all_id = format!("default-player-all-{}", kind.name());
    let label = format!("Every {} format", kind.name());
    let mut rows = vec![row(
        label,
        None,
        theme,
        Switch::new(SharedString::from(all_id), all || others.is_empty())
            .disabled(locked || others.is_empty())
            .on_toggle(cx.listener(move |view, &on: &bool, _, cx| {
                let picked = &mut view.default_player.picked;
                for extension in &others {
                    if on {
                        picked.insert(extension);
                    } else {
                        picked.remove(extension);
                    }
                }
                cx.notify();
            })),
    )];
    rows.extend(
        openers
            .iter()
            .map(|opener| format_row(opener, picked, locked, theme, cx)),
    );
    rows
}

/// A format's row: what it is, its extensions and its app now, and
/// whether it's picked for Plays, which it is for good once Plays opens it.
fn format_row(
    opener: &Opener,
    picked: &BTreeSet<&'static str>,
    locked: bool,
    theme: &Theme,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let format: &'static Format = opener.format;
    let extension = format.extension();
    let extensions: Vec<String> = format.extensions.iter().map(|e| format!(".{e}")).collect();
    let hint = format!("{} · {}", extensions.join(", "), opener.says());
    let on = opener.plays || picked.contains(extension);
    row(
        format.name,
        Some(hint.into()),
        theme,
        Switch::new(
            SharedString::from(format!("default-player-{extension}")),
            on,
        )
        .disabled(locked || opener.plays)
        .on_toggle(cx.listener(move |view, &on: &bool, _, cx| {
            let picked = &mut view.default_player.picked;
            if on {
                picked.insert(extension);
            } else {
                picked.remove(extension);
            }
            cx.notify();
        })),
    )
}

/// Read again which app opens each format, off the main thread.
fn read(view: &mut SettingsView, cx: &mut Context<SettingsView>) {
    view.default_player.reading = true;
    let reading = (System::get(cx).read)(cx);
    cx.spawn(async move |view, cx| {
        let openers = reading.await;
        view.update(cx, |view, cx| {
            view.default_player.openers = Some(openers);
            view.default_player.reading = false;
            cx.notify();
        })
        .ok();
    })
    .detach();
}

/// Ask the system for Plays to open the formats picked; then say what came
/// of it, and read each format's app again.
fn ask(view: &mut SettingsView, cx: &mut Context<SettingsView>) {
    let state = &mut view.default_player;
    let Some(Ok(openers)) = &state.openers else {
        return;
    };
    let formats: Vec<&'static Format> = openers
        .iter()
        .map(|opener| opener.format)
        .filter(|format| state.picked.contains(format.extension()))
        .collect();
    if formats.is_empty() || state.asking {
        return;
    }
    state.asking = true;
    state.made = None;
    let asking = (System::get(cx).make)(formats, cx);
    cx.spawn(async move |view, cx| {
        let made = asking.await;
        view.update(cx, |view, cx| {
            let state = &mut view.default_player;
            state.asking = false;
            state.made = Some(made.says());
            state.picked.clear();
            state.openers = None;
            cx.notify();
        })
        .ok();
    })
    .detach();
    cx.notify();
}
