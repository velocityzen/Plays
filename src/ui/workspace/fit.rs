//! Sizing a window to the videos opened in it: each at its own size, in the
//! grid's rows, but no bigger than the screen. As files open, however long
//! that takes (a network drive can take a while), and not if the window
//! was resized meanwhile; after that the window is the user's to size, or
//! to have sized so again with Video → Match Video Size.

use std::time::Instant;

use gpui::prelude::*;
use gpui::{App, Context, EntityId, Pixels, Size, Window, px, size};

use super::{TOOLBAR_SPACE, Workspace};
use crate::settings::Settings;
use crate::ui::player::PlayerView;
use crate::ui::theme::layout::{AUDIO_WINDOW_SIZE, SCREEN_MARGIN, TILE_GAP, WINDOW_MIN_SIZE};
use crate::ui::theme::motion::{FIT_POLL, FIT_WAIT};

/// Who has the window sized for its videos.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sizing {
    /// Files opening into it, which leave a window the user zoomed be.
    Opening,
    /// The user, with Video → Match Video Size: zoomed or not.
    Asked,
}

/// Whether the window may be sized for its videos: never in full screen,
/// and zoomed only when asked to.
fn may_size(sizing: Sizing, full_screen: bool, zoomed: bool) -> bool {
    !full_screen && (sizing == Sizing::Asked || !zoomed)
}

/// Files opened into a window whose sizes it waits for.
pub(super) struct Fitting {
    /// Their tiles.
    opened: Vec<EntityId>,
    /// When the last of them was opened.
    started: Instant,
}

impl Workspace {
    /// Once the tiles just `opened` know how big their videos are (or have
    /// failed, or `FIT_WAIT` has passed), size the window for every tile,
    /// unless it was resized meanwhile. Files opened while it waits are
    /// waited for too.
    pub(super) fn fit_when_known(&mut self, opened: Vec<EntityId>, cx: &mut Context<Self>) {
        if let Some(fitting) = &mut self.fitting {
            fitting.opened.extend(opened);
            fitting.started = Instant::now();
            return;
        }
        self.fitting = Some(Fitting {
            opened,
            started: Instant::now(),
        });
        let window = self.window;
        cx.spawn(async move |this, cx| {
            let Ok(size_at_open) = cx.update_window(window, |_, window, _| window.bounds().size)
            else {
                return;
            };
            loop {
                cx.background_executor().timer(FIT_POLL).await;
                let done = cx.update_window(window, |_, window, cx| {
                    this.update(cx, |this, cx| {
                        if !this.sizes_known(cx) {
                            return false;
                        }
                        this.fitting = None;
                        let size = window.bounds().size;
                        let resized = (size.width - size_at_open.width).abs() > px(1.)
                            || (size.height - size_at_open.height).abs() > px(1.);
                        if resized {
                            log::debug!("not sizing the window for its videos: it was resized");
                        } else {
                            this.fit_window(window, Sizing::Opening, cx);
                        }
                        true
                    })
                });
                if !matches!(done, Ok(Ok(false))) {
                    return;
                }
            }
        })
        .detach();
    }

    /// The files waited for know how big their videos are, or have failed,
    /// or have had `FIT_WAIT` to.
    fn sizes_known(&self, cx: &App) -> bool {
        let Some(fitting) = &self.fitting else {
            return true;
        };
        fitting.started.elapsed() >= FIT_WAIT
            || self
                .tiles
                .iter()
                .filter(|tile| fitting.opened.contains(&tile.view.entity_id()))
                .all(|tile| {
                    let player = tile.view.read(cx).player();
                    player.is_known() || player.error().is_some()
                })
    }

    /// Size the window for its videos now, as far as their sizes are
    /// known, as Video → Match Video Size does. Zoomed too, but not in full
    /// screen.
    pub fn match_video_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.fit_window(window, Sizing::Asked, cx);
    }

    /// Size the window so every tile shows its video at its own size, or
    /// as near as the screen allows, if `sizing` may.
    fn fit_window(&mut self, window: &mut Window, sizing: Sizing, cx: &mut Context<Self>) {
        let may = may_size(sizing, window.is_fullscreen(), window.is_maximized());
        if !may || self.tiles.is_empty() {
            return;
        }
        let count = self.tiles.len();
        let columns = cx.global::<Settings>().columns_for(count).clamp(1, count);
        let cells: Vec<_> = self
            .tiles
            .iter()
            .filter_map(|tile| shown_size(tile.view.read(cx)))
            .collect();
        let room = screen::room(window, cx);
        let room = size(room.width, room.height - TOOLBAR_SPACE);
        let Some(grid) = grid_size(&cells, count, columns, room) else {
            return;
        };
        let content = size(
            grid.width.max(WINDOW_MIN_SIZE.width),
            (grid.height + TOOLBAR_SPACE).max(WINDOW_MIN_SIZE.height),
        );
        log::debug!("sizing the window for its videos: {content:?}");
        screen::set_content_size(window, content, cx);
    }
}

/// How big a tile's video shows, turned and adjusted, at a point a pixel;
/// an audio file's tile takes the size its own window would have.
fn shown_size(view: &PlayerView) -> Option<Size<Pixels>> {
    if let Some(framing) = view.framing() {
        let (width, height) = framing.size;
        return Some(size(px(width), px(height)));
    }
    let info = view.player().info()?;
    (!info.has_video).then_some(AUDIO_WINDOW_SIZE)
}

/// The grid for `count` tiles in `columns` columns, whose cells all fit the
/// biggest of `cells` (the sizes known), scaled down evenly to fit `room`.
/// `None` if no size is known.
fn grid_size(
    cells: &[Size<Pixels>],
    count: usize,
    columns: usize,
    room: Size<Pixels>,
) -> Option<Size<Pixels>> {
    let cell = cells.iter().copied().reduce(|a, b| a.max(&b))?;
    let columns = columns.clamp(1, count.max(1));
    let rows = count.max(1).div_ceil(columns);
    let gaps = size(
        TILE_GAP * (columns - 1) as f32,
        TILE_GAP * (rows - 1) as f32,
    );
    let full = size(cell.width * columns as f32, cell.height * rows as f32);
    let scale = ((room.width - gaps.width) / full.width)
        .min((room.height - gaps.height) / full.height)
        .clamp(0., 1.);
    Some(size(
        (full.width * scale + gaps.width).round(),
        (full.height * scale + gaps.height).round(),
    ))
}

/// The screen the window is on, and putting the window on it.
mod screen {
    use gpui::{App, Pixels, Size, Window, size};

    use super::SCREEN_MARGIN;

    /// The most a window's content can be on its screen.
    pub fn room(window: &Window, cx: &App) -> Size<Pixels> {
        #[cfg(target_os = "macos")]
        if let Some(room) = mac::room(window) {
            return room;
        }
        let display = window
            .display(cx)
            .map_or_else(|| window.bounds().size, |display| display.bounds().size);
        // Room for the panels and docks a desktop has.
        size(
            display.width - SCREEN_MARGIN.width,
            display.height - SCREEN_MARGIN.height,
        )
    }

    /// Resize the window's content, keeping it where it was as far as the
    /// screen allows, once the update this is called in is over.
    pub fn set_content_size(window: &mut Window, content: Size<Pixels>, cx: &App) {
        #[cfg(target_os = "macos")]
        if mac::set_content_size(window, content, cx) {
            return;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = cx;
        window.resize(content);
    }

    // objc's message macros test a `cargo-clippy` feature this crate
    // doesn't have.
    #[cfg(target_os = "macos")]
    #[allow(unexpected_cfgs)]
    mod mac {
        use gpui::{App, Pixels, Size, Window, px, size};
        use objc::runtime::{Object, YES};
        use objc::{msg_send, sel, sel_impl};
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct NSRect {
            x: f64,
            y: f64,
            width: f64,
            height: f64,
        }

        fn ns_window(window: &Window) -> Option<*mut Object> {
            // gpui's own `Window::window_handle` returns something else.
            let handle = HasWindowHandle::window_handle(window).ok()?;
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                return None;
            };
            let view = handle.ns_view.as_ptr().cast::<Object>();
            // SAFETY: `view` is the window's NSView, alive as long as
            // `window`, and this runs on the main thread.
            let ns_window: *mut Object = unsafe { msg_send![view, window] };
            (!ns_window.is_null()).then_some(ns_window)
        }

        /// The screen's frame less the menu bar and the Dock, as the
        /// window's content.
        fn visible(ns_window: *mut Object) -> Option<NSRect> {
            // SAFETY: a live window, on the main thread.
            unsafe {
                let screen: *mut Object = msg_send![ns_window, screen];
                if screen.is_null() {
                    return None;
                }
                let frame: NSRect = msg_send![screen, visibleFrame];
                Some(frame)
            }
        }

        pub fn room(window: &Window) -> Option<Size<Pixels>> {
            let ns_window = ns_window(window)?;
            let visible = visible(ns_window)?;
            // SAFETY: a live window, on the main thread.
            let content: NSRect = unsafe { msg_send![ns_window, contentRectForFrameRect: visible] };
            Some(size(px(content.width as f32), px(content.height as f32)))
        }

        /// Whether it could: centered where the window was, then moved
        /// back inside the screen's visible part, animated.
        ///
        /// Animating runs right through, and AppKit tells gpui each size on
        /// the way, which gpui can't take while the window is being updated
        /// (it would keep laying the window out at its old size). So, as
        /// gpui's own `Window::resize` does, this resizes it once the update
        /// is over.
        pub fn set_content_size(window: &Window, content: Size<Pixels>, cx: &App) -> bool {
            let Some(ns_window) = ns_window(window) else {
                return false;
            };
            if visible(ns_window).is_none() {
                return false;
            }
            // SAFETY: a live window, on the main thread; released once the
            // task below is done with it.
            let _: *mut Object = unsafe { msg_send![ns_window, retain] };
            cx.foreground_executor()
                .spawn(async move {
                    animate_to(ns_window, content);
                    // SAFETY: retained above, on the main thread.
                    let _: () = unsafe { msg_send![ns_window, release] };
                })
                .detach();
            true
        }

        /// Animate `ns_window` to `content`, centered where it was, inside the
        /// screen's visible part.
        fn animate_to(ns_window: *mut Object, content: Size<Pixels>) {
            let Some(visible) = visible(ns_window) else {
                return;
            };
            // SAFETY: a window retained by the caller, on the main thread.
            unsafe {
                let old: NSRect = msg_send![ns_window, frame];
                let wanted = NSRect {
                    x: 0.,
                    y: 0.,
                    width: f64::from(content.width),
                    height: f64::from(content.height),
                };
                let mut frame: NSRect = msg_send![ns_window, frameRectForContentRect: wanted];
                frame.width = frame.width.min(visible.width);
                frame.height = frame.height.min(visible.height);
                frame.x = (old.x + (old.width - frame.width) / 2.)
                    .clamp(visible.x, visible.x + visible.width - frame.width);
                frame.y = (old.y + (old.height - frame.height) / 2.)
                    .clamp(visible.y, visible.y + visible.height - frame.height);
                let _: () = msg_send![ns_window, setFrame: frame display: YES animate: YES];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOM: Size<Pixels> = size(px(1920.), px(1080.));

    #[test]
    fn one_video_at_its_own_size() {
        let cells = [size(px(1280.), px(720.))];
        assert_eq!(grid_size(&cells, 1, 1, ROOM), Some(cells[0]));
    }

    #[test]
    fn too_big_for_the_screen_shrinks_to_fit() {
        let cells = [size(px(3840.), px(2160.))];
        assert_eq!(
            grid_size(&cells, 1, 1, ROOM),
            Some(size(px(1920.), px(1080.)))
        );
    }

    #[test]
    fn several_share_the_biggest_cell() {
        let cells = [size(px(640.), px(360.)), size(px(480.), px(360.))];
        // Two side by side, with the gap between them.
        assert_eq!(
            grid_size(&cells, 2, 2, ROOM),
            Some(size(px(640. * 2.) + TILE_GAP, px(360.)))
        );
        // Four in two rows, one size unknown yet.
        let four = grid_size(&cells, 4, 2, ROOM).unwrap();
        assert_eq!(four.height, px(720.) + TILE_GAP);
    }

    #[test]
    fn asked_a_zoomed_window_is_sized_too_but_never_one_in_full_screen() {
        use Sizing::{Asked, Opening};
        assert!(may_size(Opening, false, false));
        assert!(!may_size(Opening, false, true), "a zoomed window is let be");
        assert!(may_size(Asked, false, false));
        assert!(may_size(Asked, false, true), "asked, a zoomed one is sized");
        assert!(!may_size(Asked, true, false));
        assert!(!may_size(Opening, true, false));
    }

    #[test]
    fn nothing_known_leaves_the_window_be() {
        assert_eq!(grid_size(&[], 2, 2, ROOM), None);
    }
}
