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
    "check",
    "chevron-down",
    "chevron-right",
    "chevron-up-down",
    "close",
    "display",
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
    "play",
    "plus",
    "pop-out",
    "previous",
    "replay",
    "settings",
    "skip-back",
    "skip-forward",
    "stop",
    "subtitles",
    "volume",
    "window-max",
    "window-min",
);

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
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
