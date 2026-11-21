//! `app`: the app's own.

use crate::api::registry::Spec;
use crate::ui::about::AboutView;
use crate::ui::settings::SettingsView;

pub const COMMANDS: &[Spec] = &[
    Spec::app(
        "app.about",
        "About Plays",
        "Show Plays' version, and what it's made with.",
        AboutView::open_window,
    )
    .keywords(&["version", "credits", "licenses"]),
    Spec::app(
        "app.settings",
        "Settings…",
        "Open the Settings window, or bring it to the front.",
        SettingsView::open_window,
    )
    .keywords(&["preferences", "options", "shortcuts", "hotkeys"])
    .keys(&["secondary-,"]),
    Spec::app(
        "app.quit",
        "Quit Plays",
        "Quit, keeping where each file got to and stopping the devices cast to.",
        |cx| cx.quit(),
    )
    .keywords(&["exit"])
    .keys(&["secondary-q"]),
];
