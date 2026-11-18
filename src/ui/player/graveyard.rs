//! Frames that went off screen, kept a moment before they are released.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::Window;

use crate::media::Picture;

/// Frames that were on screen wait here for a few display frames before they
/// are released, so a frame still in flight on the GPU keeps its texture (or,
/// for a surface, its buffer isn't refilled with a newer frame).
#[derive(Clone, Default)]
pub struct Graveyard(Rc<RefCell<Vec<Buried>>>);

/// A retired frame and when it went off screen.
type Buried = (Picture, Instant);

const RELEASE_DELAY: Duration = Duration::from_millis(60);

impl Graveyard {
    pub fn bury(&self, picture: Picture) {
        self.0.borrow_mut().push((picture, Instant::now()));
    }

    /// Release frames that have been off screen long enough. Returns whether any are left.
    pub fn collect(&self, window: &mut Window) -> bool {
        let mut pictures = self.0.borrow_mut();
        pictures.retain(|(picture, buried)| {
            let expired = buried.elapsed() >= RELEASE_DELAY;
            // Images live in gpui's texture atlas until dropped from it;
            // surfaces only need their last reference to go.
            if expired && let Picture::Image(image) = picture {
                let _ = window.drop_image(image.clone());
            }
            !expired
        });
        !pictures.is_empty()
    }
}
