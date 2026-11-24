use std::borrow::Cow;

use anyhow::Result;
use gpui::{AssetSource, SharedString};

/// Icons compiled into the binary so the app needs no files next to it.
pub struct Assets;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $((concat!("icons/", $name, ".svg"), include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*
        ];
    };
}

icons!(
    "appearance",
    "cast",
    "check",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "chevron-up-down",
    "close",
    "display",
    "file",
    "file-play",
    "info",
    "keyboard",
    "link",
    "loop",
    "maximize",
    "minimize",
    "music",
    "mute",
    "next",
    "pause",
    "pip",
    "play",
    "plus",
    "pop-out",
    "previous",
    "replay",
    "screenshot",
    "search",
    "settings",
    "skip-back",
    "skip-forward",
    "stop",
    "subtitles",
    "terminal",
    "video",
    "volume",
    "window-max",
    "window-min",
);

/// The app icon's glyph, from the icon itself, for About to draw it with.
const APP_GLYPH: (&str, &[u8]) = (
    "icons/app-glyph.svg",
    include_bytes!("../assets/AppIcon.icon/Assets/icon.svg"),
);

/// The app's logo, which the empty window shows.
pub const LOGO: &str = "logo.svg";
const LOGO_SVG: (&str, &[u8]) = (LOGO, include_bytes!("../assets/icon.svg"));

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .chain([&APP_GLYPH, &LOGO_SVG])
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

#[cfg(test)]
#[path = "tests/assets.rs"]
mod tests;
