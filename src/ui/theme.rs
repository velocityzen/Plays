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

/// Text styles, by what the text is: its size and weight. Its color is up
/// to where it is.
pub mod text {
    use gpui::{FontWeight, Styled};

    pub trait TextStyle: Styled + Sized {
        /// The one big line of a screen, such as the empty window's.
        fn text_display(self) -> Self {
            self.text_xl().font_weight(FontWeight::BOLD)
        }

        /// A pane's or a dialog's title.
        fn text_title(self) -> Self {
            self.text_base().font_weight(FontWeight::BOLD)
        }

        /// A group's heading, such as a card of settings'.
        fn text_heading(self) -> Self {
            self.text_sm().font_weight(FontWeight::SEMIBOLD)
        }

        /// Small and heavy: a menu column's heading, a badge's word.
        fn text_label(self) -> Self {
            self.text_xs().font_weight(FontWeight::SEMIBOLD)
        }
    }

    impl<T: Styled> TextStyle for T {}
}

// How big things are, how round, how far apart and how long they take, by
// where they are used. Tailwind-like spacing (`p_2`, `gap_1`, `text_sm`) is
// gpui's own scale and stays in place.

/// Corner radii. They are drawn as superellipses (`.superellipse`), which
/// curve over more of the sides than a circle of the same radius would.
/// Components' come in their sizes, `[small, medium, large]`.
pub mod radius {
    use gpui::{Pixels, px};

    /// Panels over the picture; notes and the track reel are medium.
    pub const OVERLAY: [Pixels; 3] = [px(16.), px(24.), px(32.)];
    /// The controls' panel, which is short and has buttons close to its
    /// corners.
    pub const CONTROLS: Pixels = px(16.);
    /// Dialogs: Open URL.
    pub const DIALOG: Pixels = px(12.);
    /// Cards: a group of settings, the subtitle preview.
    pub const CARD: [Pixels; 3] = [px(8.), px(10.), px(12.)];
    /// Menus; cover art has a medium one's corners.
    pub const MENU: [Pixels; 3] = [px(6.), px(8.), px(10.)];
    /// Rows in the Settings sidebar.
    pub const FRAME: Pixels = px(7.);
    /// Buttons, text fields and hotkey chips; the subtitle preview's
    /// picture has a medium one's corners.
    pub const CONTROL: [Pixels; 3] = [px(5.), px(6.), px(8.)];
    /// Rows in menus, segments (their frame adds its padding to theirs),
    /// and badges with an icon.
    pub const ITEM: [Pixels; 3] = [px(4.), px(5.), px(6.)];
    /// Badges with a word, such as LIVE.
    pub const BADGE: [Pixels; 3] = [px(3.), px(4.), px(5.)];
}

/// How long things take.
pub mod motion {
    use std::time::Duration;

    /// How long a note on what a shortcut did shows.
    pub const OSD_DURATION: Duration = Duration::from_millis(1200);
    /// How long, at the end of that, it takes to fade out.
    pub const OSD_FADE: Duration = Duration::from_millis(200);
    /// How long the track reel takes to roll to the track picked.
    pub const REEL_ROLL: Duration = Duration::from_millis(220);
    /// How long the mouse can rest before a playing tile's header and
    /// controls fade (and the pointer hides), as if it had left the window.
    pub const MOUSE_IDLE: Duration = Duration::from_millis(2500);
    /// How long files just opened get to say how big their videos are
    /// (from a network drive, it can take a while) before their window is
    /// sized for the ones that have, and how often it looks.
    pub const FIT_WAIT: Duration = Duration::from_secs(30);
    pub const FIT_POLL: Duration = Duration::from_millis(50);
    /// How much of the audio level meter's reading is kept each frame as
    /// the sound gets quieter; it rises at once.
    pub const LEVEL_FALLOFF: f32 = 0.88;
}

/// Windows and how tiles fill them.
pub mod layout {
    use gpui::{Pixels, Point, Size, point, px, size};

    /// The first window, and the most a popped-out video's window opens at.
    pub const WINDOW_SIZE: Size<Pixels> = size(px(1280.), px(800.));
    /// The smallest a window gets, which fits the controls of one tile.
    pub const WINDOW_MIN_SIZE: Size<Pixels> = size(px(480.), px(320.));
    /// A popped-out audio file's window.
    pub const AUDIO_WINDOW_SIZE: Size<Pixels> = size(px(420.), px(420.));
    /// A popped-out tile's window before the size of its video is known.
    pub const VIDEO_WINDOW_SIZE: Size<Pixels> = size(px(960.), px(540.));
    /// Room a window sized for its videos leaves on its screen, for the
    /// panels and docks of desktops gpui can't ask (macOS tells).
    pub const SCREEN_MARGIN: Size<Pixels> = size(px(48.), px(96.));
    pub const SETTINGS_WINDOW_SIZE: Size<Pixels> = size(px(720.), px(520.));
    pub const SETTINGS_WINDOW_MIN_SIZE: Size<Pixels> = size(px(680.), px(420.));
    /// Where the macOS traffic lights go in windows without a title bar.
    pub const TRAFFIC_LIGHTS: Point<Pixels> = point(px(14.), px(14.));
    /// The room a bar under the traffic lights leaves on its left.
    pub const TRAFFIC_LIGHTS_WIDTH: Pixels = px(80.);
    /// The toolbar's height on Linux and Windows (macOS has none), without
    /// the 1px border under it.
    pub const TOOLBAR_HEIGHT: Pixels = px(44.);
    /// Below this window width the toolbar's buttons lose their labels.
    pub const COMPACT_WIDTH: Pixels = px(760.);
    /// The gap between tiles.
    pub const TILE_GAP: Pixels = px(2.);
    /// Around the grid while files are dragged over it.
    pub const DROP_INSET: Pixels = px(6.);
    /// The empty window's ring: its share of the window's shorter side, and
    /// its smallest and largest sizes. It is drawn 2 points wide.
    pub const RING_SCALE: f32 = 0.8;
    pub const RING_MIN_DIAMETER: Pixels = px(300.);
    pub const RING_MAX_DIAMETER: Pixels = px(480.);
    /// The least room the ring leaves around it in a small window.
    pub const RING_MARGIN: Pixels = px(16.);
    /// The ring's border, as an opacity of the text color, and of the
    /// accent while a file is dragged over it.
    pub const RING_BORDER: f32 = 0.1;
    pub const RING_BORDER_DROP: f32 = 0.6;
    /// The URL field in the ring: its share of the ring, and the least and
    /// most it is wide.
    pub const RING_FORM_SCALE: f32 = 0.62;
    pub const RING_FORM_MIN_WIDTH: Pixels = px(210.);
    pub const RING_FORM_MAX_WIDTH: Pixels = px(320.);
}

/// Menu rows, by size (the track menu's, the track reel's and the font
/// menu's are medium), and where menus go.
pub mod menu {
    use gpui::{Pixels, px};

    /// A row's height, the room on its sides, and the space for its check.
    pub const ROW_HEIGHT: [Pixels; 3] = [px(22.), px(26.), px(30.)];
    pub const ROW_PADDING: [Pixels; 3] = [px(6.), px(8.), px(10.)];
    pub const CHECK_WIDTH: [Pixels; 3] = [px(12.), px(14.), px(16.)];
    /// A row's detail while the row is hovered, as an opacity of white.
    pub const DETAIL_HOVER: f32 = 0.8;
    /// How far a menu keeps from the window's edges.
    pub const WINDOW_MARGIN: Pixels = px(8.);
}

/// The components in `components` and the text field, by size: each list
/// is `[small, medium, large]`, and medium is the default.
pub mod controls {
    use gpui::{Pixels, Size, px, size};

    /// Buttons, text fields, segmented controls and hotkey chips. An icon
    /// button is a square this high.
    pub const HEIGHT: [Pixels; 3] = [px(22.), px(28.), px(34.)];
    /// The room on their sides, and on a wide button's.
    pub const PADDING: [Pixels; 3] = [px(8.), px(10.), px(12.)];
    pub const WIDE_PADDING: [Pixels; 3] = [px(12.), px(16.), px(20.)];
    /// An icon next to a button's label.
    pub const BUTTON_ICON: [Pixels; 3] = [px(12.), px(14.), px(16.)];
    /// An icon on its own, as on an icon button, unless it is given a size.
    pub const ICON: [Pixels; 3] = [px(12.), px(16.), px(20.)];
    /// A filled button's (primary, danger) background when hovered and
    /// pressed, as an opacity of its color, and its shortcut's text, as one
    /// of white.
    pub const FILLED_HOVER: f32 = 0.85;
    pub const FILLED_ACTIVE: f32 = 0.7;
    pub const FILLED_SHORTCUT: f32 = 0.7;
    /// A secondary button's tint of the accent, as its opacity: at rest,
    /// hovered, and pressed or selected.
    pub const TINT: f32 = 0.12;
    pub const TINT_HOVER: f32 = 0.18;
    pub const TINT_ACTIVE: f32 = 0.26;
    /// A disabled component's opacity.
    pub const DISABLED: f32 = 0.45;
    pub const SLIDER_HEIGHT: [Pixels; 3] = [px(12.), px(16.), px(20.)];
    pub const SLIDER_MIN_WIDTH: Pixels = px(32.);
    pub const SLIDER_KNOB: [Pixels; 3] = [px(8.), px(10.), px(14.)];
    pub const SLIDER_KNOB_DRAGGED: [Pixels; 3] = [px(10.), px(12.), px(16.)];
    /// A meter's bar, which is also a slider's track.
    pub const METER_HEIGHT: [Pixels; 3] = [px(3.), px(4.), px(6.)];
    /// A switch, and the room around its knob, which fills the rest of its
    /// height.
    pub const SWITCH: [Size<Pixels>; 3] = [
        size(px(28.), px(16.)),
        size(px(36.), px(20.)),
        size(px(44.), px(24.)),
    ];
    pub const SWITCH_PADDING: Pixels = px(2.);
    /// The padding in a segmented control, and the gap between segments,
    /// which fill the rest of its height.
    pub const SEGMENTED_PADDING: Pixels = px(2.);
    /// A badge with an icon, which is square, and its icon; the room on a
    /// badge with a word's sides.
    pub const BADGE: [Pixels; 3] = [px(16.), px(20.), px(24.)];
    pub const BADGE_ICON: [Pixels; 3] = [px(10.), px(12.), px(14.)];
    pub const BADGE_PADDING: [Pixels; 3] = [px(4.), px(6.), px(8.)];
    pub const CARET_WIDTH: Pixels = px(1.5);
    /// The underline under text an input method is still composing.
    pub const COMPOSING_UNDERLINE: Pixels = px(1.);
    /// How close to a text field's right edge the caret may get before the
    /// text scrolls.
    pub const TEXT_SCROLL_MARGIN: Pixels = px(2.);
    /// Selected text's background, as an opacity of the accent.
    pub const SELECTION: f32 = 0.3;
}

/// A player's tile.
pub mod player {
    use gpui::{Pixels, Point, point, px};

    /// How far the controls' panel floats from the tile's sides and bottom.
    pub const CONTROLS_INSET: Pixels = px(8.);
    /// The padding in the controls' panel.
    pub const CONTROLS_PADDING: Pixels = px(6.);
    /// The gap between the seek bar and the buttons under it.
    pub const CONTROLS_GAP: Pixels = px(2.);
    /// The volume slider's width.
    pub const VOLUME_WIDTH: Pixels = px(72.);
    /// Where the track menu goes from the top right of its button.
    pub const TRACK_MENU_OFFSET: Point<Pixels> = point(px(28.), px(-6.));
    /// A column of the track menu, and the track reel.
    pub const TRACK_MENU_WIDTH: Pixels = px(240.);
    /// The most a column of the track menu is tall; it scrolls beyond.
    pub const TRACK_MENU_MAX_HEIGHT: Pixels = px(320.);
    /// How bright the tracks before and after the one picked are on the reel.
    pub const REEL_FADED: f32 = 0.4;
    /// How far from the tile's top notes show, below the title bar.
    pub const NOTE_TOP: Pixels = px(44.);
    /// The most an error message is wide.
    pub const ERROR_MAX_WIDTH: Pixels = px(420.);
    /// Cover art, and the level meter under it.
    pub const COVER_SIZE: Pixels = px(160.);
    /// The circle standing in for missing cover art, and its icon.
    pub const NO_COVER_SIZE: Pixels = px(96.);
    pub const NO_COVER_ICON: Pixels = px(40.);
    /// The icon in a tile whose video is in Picture in Picture.
    pub const PIP_ICON: Pixels = px(40.);
}

/// The console over a tile (`~`).
pub mod console {
    use std::time::Duration;

    use gpui::{Pixels, px};

    /// Where it sits in the tile: under the header, on the left.
    pub const TOP: Pixels = px(44.);
    pub const INSET: Pixels = px(12.);
    /// The labels' column, and the most it is wide.
    pub const LABEL_WIDTH: Pixels = px(72.);
    pub const MAX_WIDTH: Pixels = px(560.);
    /// Monospaced, so the numbers stay put as they change.
    #[cfg(target_os = "macos")]
    pub const FONT: &str = "Menlo";
    #[cfg(not(target_os = "macos"))]
    pub const FONT: &str = "DejaVu Sans Mono";
    /// How often it updates while nothing else redraws the tile.
    pub const REFRESH: Duration = Duration::from_millis(250);
}

/// The Settings window.
pub mod settings {
    use gpui::{Pixels, Size, point, px, size};

    pub const SIDEBAR_WIDTH: Pixels = px(200.);
    /// Room above the sidebar's first row: under the traffic lights on
    /// macOS, which sit over the sidebar.
    pub const SIDEBAR_TOP_MAC: Pixels = px(48.);
    pub const SIDEBAR_TOP: Pixels = px(10.);
    pub const SIDEBAR_ROW_HEIGHT: Pixels = px(30.);
    /// The sections' badges' colors, macOS's system colors as System Settings has
    /// them.
    pub const PLAYBACK_COLOR: u32 = 0x34c759;
    pub const AUDIO_COLOR: u32 = 0xff375f;
    pub const VIDEO_COLOR: u32 = 0x0a84ff;
    pub const SUBTITLES_COLOR: u32 = 0xff9f0a;
    pub const SCREENSHOTS_COLOR: u32 = 0xbf5af2;
    pub const HOTKEYS_COLOR: u32 = 0x5e5ce6;
    pub const APPEARANCE_COLOR: u32 = 0x8e8e93;
    pub const ABOUT_COLOR: u32 = 0x30b0c7;
    /// The settings file's badge, at the foot of the sidebar.
    pub const FILE_COLOR: u32 = 0x636366;
    /// The settings file's menu, and how far above its row it opens.
    pub const FILE_MENU_WIDTH: Pixels = px(280.);
    pub const FILE_MENU_GAP: Pixels = px(4.);
    /// The chevron on the settings file's row.
    pub const FILE_CHEVRON: Pixels = px(12.);
    /// The pane's title bar, in line with the traffic lights on macOS.
    pub const TITLE_HEIGHT: Pixels = px(42.);
    /// A slider with its value next to it, and the value's width.
    pub const SLIDER_WIDTH: Pixels = px(200.);
    pub const SLIDER_VALUE_WIDTH: Pixels = px(40.);
    /// A color swatch, and the room between it and the ring round the one
    /// picked.
    pub const SWATCH: Pixels = px(26.);
    pub const SWATCH_PADDING: Pixels = px(2.);
    /// The most the audio output's name is wide.
    pub const OUTPUT_MAX_WIDTH: Pixels = px(260.);
    /// The audio output button, which its menu lines up with.
    pub const DEVICE_BUTTON_WIDTH: Pixels = px(220.);
    /// How far below the audio output button's top its menu goes.
    pub const DEVICE_MENU_TOP: Pixels = px(32.);
    pub const DEVICE_MENU_WIDTH: Pixels = px(300.);
    /// The most the menu of audio outputs is tall before it scrolls.
    pub const DEVICE_MENU_MAX_HEIGHT: Pixels = px(320.);
    /// The subtitle font button, which its menu lines up with.
    pub const FONT_BUTTON_WIDTH: Pixels = px(200.);
    /// How far below the font button's top its menu goes.
    pub const FONT_MENU_TOP: Pixels = px(32.);
    pub const FONT_MENU_WIDTH: Pixels = px(260.);
    pub const FONT_MENU_HEIGHT: Pixels = px(280.);
    /// The most a command's hotkey chips take before wrapping.
    pub const HOTKEY_KEYS_MAX_WIDTH: Pixels = px(280.);
    /// The most a credit's license takes in About, such as "MIT OR
    /// Apache-2.0".
    pub const CREDIT_LICENSE_MAX_WIDTH: Pixels = px(220.);
    /// The subtitle preview's tile, 16:9.
    pub const PREVIEW_SIZE: Size<Pixels> = size(px(284.), px(160.));
    /// The shape of the picture in the subtitle preview: a wide film, so it
    /// shows the bars that "Place on: Tile" lets subtitles go in.
    pub const PREVIEW_PICTURE: (u32, u32) = (2390, 1000);
    /// The preview's dusk sky, from the top down, bright near the bottom
    /// where the subtitles go, so the edge settings show.
    pub const PREVIEW_SKY: (u32, u32) = (0x1b2a4a, 0xf4a261);
    /// The sun in it: its color, its size as a share of the picture's
    /// height, and where its center is as shares of the picture's size.
    pub const PREVIEW_SUN: u32 = 0xffd58a;
    pub const PREVIEW_SUN_SIZE: f32 = 0.32;
    pub const PREVIEW_SUN_CENTER: gpui::Point<f32> = point(0.76, 0.36);
}

/// The Open URL dialog.
pub mod dialog {
    use gpui::{Pixels, px};

    pub const WIDTH: Pixels = px(520.);
    /// How far below the window's top it sits.
    pub const TOP: Pixels = px(96.);
    /// How dark the window behind it gets.
    pub const BACKDROP: f32 = 0.45;
}

/// Subtitles. Sizes in `em` are shares of the subtitles' font size.
pub mod subtitles {
    /// The font size at 100%, as a share of the picture's visible height,
    /// and the least and most it is in pixels.
    pub const FONT_SIZE: f32 = 0.05;
    pub const MIN_FONT_SIZE: f32 = 13.;
    pub const MAX_FONT_SIZE: f32 = 48.;
    /// Line height, in em.
    pub const LINE_HEIGHT: f32 = 1.2;
    /// How much of the width lines wrap at.
    pub const WRAP_WIDTH: f32 = 0.9;
    /// The least room under subtitles at the bottom, in pixels.
    pub const MIN_BOTTOM: f32 = 4.;
    /// The room between subtitles and the controls when they show, in em.
    pub const ABOVE_CONTROLS: f32 = 0.25;
    /// The outline's or shadow's opacity and width (in em, at least a pixel),
    /// and how far the shadow is down and right, in widths.
    pub const EDGE_OPACITY: f32 = 0.85;
    pub const EDGE_WIDTH: f32 = 0.075;
    pub const SHADOW_OFFSET: f32 = 1.3;
    /// The background box's padding either side and corner radius, in em.
    pub const BACKGROUND_PADDING: f32 = 0.3;
    pub const BACKGROUND_RADIUS: f32 = 0.12;
    /// Screenshots lay subtitles out as on a tile showing just the picture,
    /// this tall: tall enough that the font size isn't held to its least
    /// or most, so it is `FONT_SIZE` of the picture.
    pub const TEXT_HEIGHT: f32 = 720.;
}
