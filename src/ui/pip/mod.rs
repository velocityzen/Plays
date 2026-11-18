//! Native Picture in Picture, on macOS 12 and later: a tile's video in the
//! system's own floating window, over every app and Space, with its play,
//! pause and skip buttons. Plays decodes the video itself, so the frames the
//! tile would show go to an `AVSampleBufferDisplayLayer`, one by one as they
//! are due, which AVKit shows in that window (`mac`), with the subtitles
//! showing burned in (`burn_in`). Elsewhere there is no Picture in Picture,
//! and `Pip::new` says so.

use std::sync::Arc;

#[cfg(not(target_os = "macos"))]
use futures::channel::mpsc::UnboundedSender;
#[cfg(not(target_os = "macos"))]
use gpui::{Bounds, Pixels, Window};
use gpui::{ContentTransform, RenderImage, TextSystem};

#[cfg(not(target_os = "macos"))]
use crate::media::{Orientation, Picture};
use crate::ui::overlay::Paint;

#[cfg(target_os = "macos")]
mod burn_in;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::{Pip, supported};

/// What the Picture in Picture window asks of its tile, or says happened.
// Only macOS's sends them, and reads the subtitles.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug)]
pub enum PipEvent {
    /// Its play or pause button.
    SetPlaying(bool),
    /// Its skip buttons: seek this many seconds.
    Skip(f64),
    /// It is showing the video.
    Started,
    /// It closed, or handed the video back.
    Stopped,
    /// It couldn't open.
    Failed(String),
    /// Its button to go back: bring the tile's window forward.
    Restore,
}

/// Where playback is, for the window's buttons and progress.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Playback {
    pub playing: bool,
    pub position: f64,
    /// `None` while it isn't known, and for live streams, which can't be
    /// skipped through.
    pub duration: Option<f64>,
}

/// The subtitles showing, for frames of `size` pixels as they show (turned).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct Subtitles {
    pub size: (u32, u32),
    /// Text, laid out over a picture `TEXT_HEIGHT` tall.
    pub text: Vec<Paint>,
    /// Bitmaps, where they go on the picture as it shows and how to sample
    /// them there.
    pub bitmaps: Vec<(Arc<RenderImage>, [f32; 4], ContentTransform)>,
    pub text_system: Arc<TextSystem>,
}

/// Picture in Picture isn't there on this system.
#[cfg(not(target_os = "macos"))]
pub fn supported() -> bool {
    false
}

/// Stands in for macOS's, which this system doesn't have.
#[cfg(not(target_os = "macos"))]
pub struct Pip;

#[cfg(not(target_os = "macos"))]
impl Pip {
    pub fn new(
        _window: &Window,
        _frame: Bounds<Pixels>,
        _live: bool,
        _events: UnboundedSender<PipEvent>,
    ) -> anyhow::Result<Self> {
        anyhow::bail!("there is no Picture in Picture on this system")
    }

    pub fn show(&mut self, _picture: &Picture, _orientation: Orientation) {}

    pub fn update(&mut self, _playback: Playback) {}

    pub fn start_when_possible(&mut self) -> bool {
        false
    }

    pub fn set_layer_visible(&self, _visible: bool) {}

    pub fn follow_window(&mut self) {}

    pub fn subtitle_size(&self, picture: (u32, u32)) -> (u32, u32) {
        picture
    }

    pub fn set_subtitles(&mut self, _subtitles: Option<Subtitles>) {}
}
