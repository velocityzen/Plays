//! macOS's own colors, read from AppKit: the window background, label and
//! separator colors, the user's accent color and the system red.

use super::theme::Palette;

/// The system's colors as they look in dark or light mode, or `None` where
/// they can't be read (and on other systems).
#[cfg(not(target_os = "macos"))]
pub fn palette(_dark: bool) -> Option<Palette> {
    None
}

#[cfg(target_os = "macos")]
pub use mac::palette;

// objc's message macros test a `cargo-clippy` feature this crate doesn't have.
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod mac {
    use gpui::{Hsla, Rgba};
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    use super::Palette;

    type Id = *mut Object;

    /// The system's colors as they look in dark or light mode.
    pub fn palette(dark: bool) -> Option<Palette> {
        let name = if dark {
            c"NSAppearanceNameDarkAqua"
        } else {
            c"NSAppearanceNameAqua"
        };
        // SAFETY: class messages AppKit answers on any thread. The appearance
        // that system colors resolve against is set for this thread only, and
        // put back before returning.
        unsafe {
            let name: Id = msg_send![class!(NSString), stringWithUTF8String: name.as_ptr()];
            let appearance: Id = msg_send![class!(NSAppearance), appearanceNamed: name];
            if appearance.is_null() {
                return None;
            }
            let previous: Id = msg_send![class!(NSAppearance), currentAppearance];
            let _: () = msg_send![class!(NSAppearance), setCurrentAppearance: appearance];
            let palette = read_palette();
            let _: () = msg_send![class!(NSAppearance), setCurrentAppearance: previous];
            palette
        }
    }

    fn read_palette() -> Option<Palette> {
        // SAFETY: each of these NSColor class methods returns a color.
        let (background, text, text_muted, border, accent, danger): (Id, Id, Id, Id, Id, Id) = unsafe {
            (
                msg_send![class!(NSColor), windowBackgroundColor],
                msg_send![class!(NSColor), labelColor],
                msg_send![class!(NSColor), secondaryLabelColor],
                msg_send![class!(NSColor), separatorColor],
                msg_send![class!(NSColor), controlAccentColor],
                msg_send![class!(NSColor), systemRedColor],
            )
        };
        Some(Palette {
            background: srgb(background)?,
            text: srgb(text)?,
            text_muted: srgb(text_muted)?,
            border: srgb(border)?,
            accent: srgb(accent)?,
            danger: srgb(danger)?,
        })
    }

    /// A color's sRGB value. Converting a system color resolves it for the
    /// current appearance.
    fn srgb(color: Id) -> Option<Hsla> {
        if color.is_null() {
            return None;
        }
        // SAFETY: `color` is an NSColor; colorUsingColorSpace: returns nil for
        // colors that have no sRGB form, such as patterns.
        unsafe {
            let space: Id = msg_send![class!(NSColorSpace), sRGBColorSpace];
            let color: Id = msg_send![color, colorUsingColorSpace: space];
            if color.is_null() {
                return None;
            }
            let r: f64 = msg_send![color, redComponent];
            let g: f64 = msg_send![color, greenComponent];
            let b: f64 = msg_send![color, blueComponent];
            let a: f64 = msg_send![color, alphaComponent];
            Some(
                Rgba {
                    r: r as f32,
                    g: g as f32,
                    b: b as f32,
                    a: a as f32,
                }
                .into(),
            )
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "tests/system_colors.rs"]
mod tests;
