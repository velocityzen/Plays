//! The About window: the app's icon, name, version and the day it was
//! built, in a small window of its own, as macOS apps have it.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    App, Bounds, Context, Global, Image, TitlebarOptions, Window, WindowBounds, WindowHandle,
    WindowOptions, div, img, linear_color_stop, linear_gradient, rgb, svg, white,
};

use super::displays;
use super::theme::Theme;
use super::theme::about::{ICON, ICON_BOTTOM, ICON_GLYPH, ICON_RADIUS, ICON_TOP, WINDOW_SIZE};
use super::theme::layout::TRAFFIC_LIGHTS;
use super::theme::text::TextStyle;
use super::workspace::float_with_players;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// "Version 0.1.0".
pub fn version() -> &'static str {
    concat!("Version ", env!("CARGO_PKG_VERSION"))
}

/// The day the app was built, as "18 November 2026".
pub fn release_date() -> String {
    spell_date(env!("PLAYS_BUILD_DATE"))
}

/// "2026-11-18" as "18 November 2026"; as it is if it isn't a date.
fn spell_date(date: &str) -> String {
    let mut parts = date.split('-').map(str::parse::<usize>);
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Ok(year)), Some(Ok(month @ 1..=12)), Some(Ok(day))) => {
            format!("{day} {} {year}", MONTHS[month - 1])
        }
        _ => date.to_owned(),
    }
}

pub struct AboutView {
    /// The icon the app bundle has, when it runs from one.
    icon: Option<Arc<Image>>,
}

/// The About window, once it has been opened.
struct AboutWindow(WindowHandle<AboutView>);
impl Global for AboutWindow {}

impl AboutView {
    /// Bring the About window to the front, opening it if it isn't open,
    /// on the display in use (`displays::in_use`).
    pub fn open_window(cx: &mut App) {
        if let Some(handle) = cx.try_global::<AboutWindow>().map(|open| open.0)
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let mac = cfg!(target_os = "macos");
        let display = displays::in_use(cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    display,
                    WINDOW_SIZE,
                    cx,
                ))),
                display_id: display,
                titlebar: Some(TitlebarOptions {
                    title: Some("About Plays".into()),
                    appears_transparent: mac,
                    traffic_light_position: mac.then_some(TRAFFIC_LIGHTS),
                }),
                is_resizable: false,
                is_minimizable: false,
                app_id: Some("plays".into()),
                focus: true,
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Self {
                    icon: bundle_icon(),
                })
            },
        );
        match opened {
            Ok(handle) => {
                cx.set_global(AboutWindow(handle));
                float_with_players(cx);
            }
            Err(err) => log::error!("could not open About: {err:#}"),
        }
    }
}

impl Render for AboutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(window, cx);
        let icon = match &self.icon {
            Some(icon) => img(icon.clone()).size(ICON).into_any_element(),
            None => drawn_icon().into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .bg(theme.background)
            .text_color(theme.text)
            .font_family(theme.font.clone())
            .child(icon)
            .child(div().pt_3().text_display().child("Plays"))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.text_muted)
                    .child(version()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.text_muted)
                    .child(release_date()),
            )
    }
}

/// The app icon as Icon Composer draws it: its glyph on a gradient, in a
/// rounded square.
fn drawn_icon() -> impl IntoElement {
    div()
        .size(ICON)
        .flex()
        .items_center()
        .justify_center()
        .superellipse(ICON_RADIUS)
        .bg(linear_gradient(
            180.,
            linear_color_stop(rgb(ICON_TOP), 0.),
            linear_color_stop(rgb(ICON_BOTTOM), 1.),
        ))
        .child(
            svg()
                .path("icons/app-glyph.svg")
                .size(ICON * ICON_GLYPH)
                .text_color(white()),
        )
}

/// The icon of the app bundle Plays runs from, as a PNG; `None` when it
/// runs from elsewhere (a build), whose icon is the system's generic one.
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
fn bundle_icon() -> Option<Arc<Image>> {
    use std::ffi::c_void;

    use gpui::ImageFormat;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    use super::theme::about::ICON_PIXELS;

    #[repr(C)]
    struct NSRect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    }
    const PNG: usize = 4;

    let exe = std::env::current_exe().ok()?;
    if !exe.to_string_lossy().contains(".app/Contents/MacOS/") {
        return None;
    }
    let nil = std::ptr::null_mut::<Object>();
    // SAFETY: AppKit calls on the main thread, each object checked before
    // use; `data`'s bytes are copied before it is released with the pool.
    let png = unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let icon: *mut Object = msg_send![app, applicationIconImage];
        if icon.is_null() {
            return None;
        }
        let mut rect = NSRect {
            x: 0.,
            y: 0.,
            width: ICON_PIXELS,
            height: ICON_PIXELS,
        };
        let image: *const c_void =
            msg_send![icon, CGImageForProposedRect: &mut rect context: nil hints: nil];
        if image.is_null() {
            return None;
        }
        let rep: *mut Object = msg_send![class!(NSBitmapImageRep), alloc];
        let rep: *mut Object = msg_send![rep, initWithCGImage: image];
        if rep.is_null() {
            return None;
        }
        let properties: *mut Object = msg_send![class!(NSDictionary), dictionary];
        let data: *mut Object = msg_send![rep, representationUsingType: PNG properties: properties];
        let png = if data.is_null() {
            None
        } else {
            let length: usize = msg_send![data, length];
            let bytes: *const u8 = msg_send![data, bytes];
            Some(std::slice::from_raw_parts(bytes, length).to_vec())
        };
        let _: () = msg_send![rep, release];
        png?
    };
    Some(Arc::new(Image::from_bytes(ImageFormat::Png, png)))
}

#[cfg(not(target_os = "macos"))]
fn bundle_icon() -> Option<Arc<Image>> {
    None
}

#[cfg(test)]
#[path = "tests/about.rs"]
mod tests;
