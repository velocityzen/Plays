use gpui::{App, Global, Hsla, SharedString, Window, WindowAppearance, black, rgb, white};

use super::system_colors;
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

/// The colors the rest of a theme is worked out from.
#[derive(Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub border: Hsla,
    pub accent: Hsla,
    pub danger: Hsla,
}

impl Palette {
    /// macOS's standard colors, for when the system's own can't be read.
    fn standard(dark: bool) -> Self {
        if dark {
            Self {
                background: rgb(0x1e1e1e).into(),
                text: white().opacity(0.85),
                text_muted: white().opacity(0.55),
                border: white().opacity(0.1),
                accent: rgb(0x0a84ff).into(),
                danger: rgb(0xff453a).into(),
            }
        } else {
            Self {
                background: rgb(0xececec).into(),
                text: black().opacity(0.85),
                text_muted: black().opacity(0.5),
                border: black().opacity(0.1),
                accent: rgb(0x007aff).into(),
                danger: rgb(0xff3b30).into(),
            }
        }
    }
}

/// macOS's own colors for both appearances, with the user's accent color.
struct SystemPalette {
    light: Option<Palette>,
    dark: Option<Palette>,
}

impl Global for SystemPalette {}

/// Read the system's colors again; windows redraw if they changed. Called at
/// startup and whenever a window becomes active, which is when a change made
/// in System Settings (such as the accent color) shows up.
pub fn refresh_system_colors(cx: &mut App) {
    let palette = SystemPalette {
        light: system_colors::palette(false),
        dark: system_colors::palette(true),
    };
    let changed = cx
        .try_global::<SystemPalette>()
        .is_none_or(|old| old.light != palette.light || old.dark != palette.dark);
    if changed {
        cx.set_global(palette);
        cx.refresh_windows();
    }
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
        let system = cx
            .try_global::<SystemPalette>()
            .and_then(|system| if dark { system.dark } else { system.light });
        Self::new(
            font,
            system.unwrap_or_else(|| Palette::standard(dark)),
            dark,
        )
    }

    fn new(font: SharedString, palette: Palette, dark: bool) -> Self {
        let Palette {
            background,
            text,
            text_muted,
            border,
            accent,
            danger,
        } = palette;
        // Cards and buttons: a lighter shade in dark mode. In light mode,
        // white on a grey window, and a faint grey on a white one (macOS 26).
        let surface = if dark {
            background.blend(white().opacity(0.07))
        } else if background.l < 0.97 {
            white()
        } else {
            background.blend(text.opacity(0.04))
        };
        Self {
            font,
            background,
            surface,
            surface_hover: surface.blend(text.opacity(0.07)),
            surface_active: surface.blend(text.opacity(0.12)),
            border,
            text,
            text_muted,
            accent,
            danger,
            track: text.opacity(0.2),
            tile: background.blend(text.opacity(0.04)),
            drop_highlight: accent.opacity(0.15),
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
