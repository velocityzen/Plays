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
    "chevron-up-down",
    "close",
    "display",
    "file",
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
    "settings",
    "skip-back",
    "skip-forward",
    "stop",
    "subtitles",
    "terminal",
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
mod tests {
    use super::*;

    /// An icon left out of `icons!` draws as nothing.
    #[test]
    fn every_icon_is_built_in() {
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
        let mut names: Vec<String> = std::fs::read_dir(folder)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".svg").map(str::to_owned)
            })
            .collect();
        names.sort();
        assert!(!names.is_empty());
        for name in names {
            assert!(
                Assets.load(&format!("icons/{name}.svg")).unwrap().is_some(),
                "assets/icons/{name}.svg isn't in `icons!`"
            );
        }
    }

    #[test]
    fn the_logo_is_built_in() {
        let logo = Assets.load(LOGO).unwrap().expect("the logo");
        assert!(logo.starts_with(b"<svg"), "assets/icon.svg");
    }
}
