//! Fading the macOS traffic lights in and out with the video controls. gpui
//! draws them over the video but has no way to hide them, so this goes to the
//! window's standard buttons directly.

use gpui::Window;

/// Fade the window's traffic lights in or out. Does nothing on other systems.
pub fn set_visible(window: &Window, visible: bool) {
    #[cfg(target_os = "macos")]
    mac::set_visible(window, visible);
    #[cfg(not(target_os = "macos"))]
    let _ = (window, visible);
}

// objc's message macros test a `cargo-clippy` feature this crate doesn't have.
#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod mac {
    use gpui::Window;
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    /// NSWindowCloseButton, NSWindowMiniaturizeButton and NSWindowZoomButton.
    const BUTTONS: [usize; 3] = [0, 1, 2];

    pub fn set_visible(window: &Window, visible: bool) {
        // gpui's own `Window::window_handle` returns something else.
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return;
        };
        let view = handle.ns_view.as_ptr().cast::<Object>();
        let alpha: f64 = if visible { 1.0 } else { 0.0 };
        // SAFETY: `view` is the window's NSView, alive as long as `window`,
        // and gpui renders on the main thread, where AppKit has to be used.
        unsafe {
            let ns_window: *mut Object = msg_send![view, window];
            if ns_window.is_null() {
                return;
            }
            for kind in BUTTONS {
                let button: *mut Object = msg_send![ns_window, standardWindowButton: kind];
                if button.is_null() {
                    continue;
                }
                // Through the animator, the change fades in or out.
                let animator: *mut Object = msg_send![button, animator];
                let _: () = msg_send![animator, setAlphaValue: alpha];
            }
        }
    }
}
