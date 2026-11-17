use gpui::{App, Global, Hsla, SharedString, Window, WindowAppearance, hsla, rgb};

use crate::settings::{Settings, ThemeMode};

/// The UI font picked at startup (see [`pick_ui_font`]).
pub struct UiFont(pub SharedString);

impl Global for UiFont {}

/// gpui's default font is IBM Plex Sans on Linux, which most systems don't ship,
/// so pick something that is actually installed.
pub fn pick_ui_font(cx: &App) -> SharedString {
    if cfg!(target_os = "macos") {
        return ".SystemUIFont".into();
    }
    let installed = cx.text_system().all_font_names();
    const PREFERRED: &[&str] = &[
        "Inter",
        "Cantarell",
        "Noto Sans",
        "Ubuntu",
        "Roboto",
        "Open Sans",
        "DejaVu Sans",
        "Liberation Sans",
        "IBM Plex Sans",
        "FreeSans",
    ];
    PREFERRED
        .iter()
        .find(|name| installed.iter().any(|n| n == *name))
        .map(|name| SharedString::from(*name))
        .or_else(|| {
            installed
                .into_iter()
                .find(|n| !n.starts_with('.'))
                .map(Into::into)
        })
        .unwrap_or_else(|| ".SystemUIFont".into())
}

#[derive(Clone)]
pub struct Theme {
    pub font: SharedString,
    pub background: Hsla,
    pub surface: Hsla,
    pub surface_hover: Hsla,
    pub surface_active: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub accent: Hsla,
    pub danger: Hsla,
    pub track: Hsla,
    pub tile: Hsla,
    pub drop_highlight: Hsla,
}

impl Theme {
    pub fn get(window: &Window, cx: &App) -> Self {
        let dark = match cx.global::<Settings>().theme {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        };
        let font = cx.global::<UiFont>().0.clone();
        if dark {
            Self::dark(font)
        } else {
            Self::light(font)
        }
    }

    fn dark(font: SharedString) -> Self {
        Self {
            font,
            background: rgb(0x0e1014).into(),
            surface: rgb(0x171a21).into(),
            surface_hover: rgb(0x222733).into(),
            surface_active: rgb(0x2b3140).into(),
            border: rgb(0x262b36).into(),
            text: rgb(0xe8eaf0).into(),
            text_muted: rgb(0x8d95a8).into(),
            accent: rgb(0x6b8cff).into(),
            danger: rgb(0xff6b7d).into(),
            track: rgb(0x343a48).into(),
            tile: rgb(0x14171d).into(),
            drop_highlight: hsla(0.63, 0.9, 0.65, 0.12),
        }
    }

    fn light(font: SharedString) -> Self {
        Self {
            font,
            background: rgb(0xeef0f4).into(),
            surface: rgb(0xffffff).into(),
            surface_hover: rgb(0xe6e9f0).into(),
            surface_active: rgb(0xdadfe9).into(),
            border: rgb(0xd5d9e2).into(),
            text: rgb(0x1b1e26).into(),
            text_muted: rgb(0x687087).into(),
            accent: rgb(0x3d63e6).into(),
            danger: rgb(0xd92d4a).into(),
            track: rgb(0xcfd4de).into(),
            tile: rgb(0xe2e5eb).into(),
            drop_highlight: hsla(0.63, 0.8, 0.5, 0.10),
        }
    }
}

/// Colors for controls drawn on top of video, which is dark in both themes.
pub mod overlay {
    use gpui::{Hsla, hsla};

    pub fn text() -> Hsla {
        hsla(0.0, 0.0, 1.0, 0.95)
    }
    pub fn text_muted() -> Hsla {
        hsla(0.0, 0.0, 1.0, 0.7)
    }
    pub fn hover() -> Hsla {
        hsla(0.0, 0.0, 1.0, 0.16)
    }
    pub fn track() -> Hsla {
        hsla(0.0, 0.0, 1.0, 0.28)
    }
    pub fn scrim() -> Hsla {
        hsla(0.0, 0.0, 0.0, 0.62)
    }
    pub fn scrim_clear() -> Hsla {
        hsla(0.0, 0.0, 0.0, 0.0)
    }
}
