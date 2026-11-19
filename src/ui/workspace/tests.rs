//! A workspace window on gpui's test platform: no screen, simulated input,
//! and a clock that moves only when told. Files still decode on threads of
//! their own, so waiting for them takes real time.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{TestAppContext, WindowHandle, px, size};

use super::{TOOLBAR, Workspace};
use crate::app;
use crate::ui::theme::layout::{TOOLBAR_HEIGHT, WINDOW_MIN_SIZE, WINDOW_SIZE};
use crate::ui::theme::motion::FIT_POLL;

/// How long a file in `tests/fixtures` may take to open.
const OPEN_WAIT: Duration = Duration::from_secs(10);

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// The app as it starts, and an empty window.
fn open_window(cx: &mut TestAppContext) -> WindowHandle<Workspace> {
    cx.update(app::set_up_for_tests);
    cx.update(|cx| Workspace::open_window(WINDOW_SIZE, cx))
        .expect("opening a window")
}

/// Run what's ready until `done`, giving decoding threads real time, for
/// at most `OPEN_WAIT`.
fn wait_until(cx: &mut TestAppContext, mut done: impl FnMut(&mut TestAppContext) -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < OPEN_WAIT {
        cx.run_until_parked();
        if done(cx) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[gpui::test]
fn opens_a_file_in_a_tile_and_fits_the_window_to_it(cx: &mut TestAppContext) {
    let window = open_window(cx);
    window
        .update(cx, |workspace, _, cx| {
            workspace.open_paths(vec![fixture("clip.mp4")], cx)
        })
        .unwrap();

    let opened = wait_until(cx, |cx| {
        window
            .read_with(cx, |workspace, cx| {
                workspace
                    .tiles
                    .iter()
                    .any(|tile| tile.view.read(cx).player().info().is_some())
            })
            .unwrap()
    });
    assert!(opened, "clip.mp4 didn't open in {OPEN_WAIT:?}");
    let info = window
        .read_with(cx, |workspace, cx| {
            assert_eq!(workspace.tiles.len(), 1);
            workspace.tiles[0].view.read(cx).player().info()
        })
        .unwrap()
        .unwrap();
    assert_eq!(info.video_size, Some((160, 120)));
    assert!(info.has_audio);

    // The window waits for the video's size, then fits it: at its
    // smallest here, as the video is smaller still.
    cx.executor().advance_clock(FIT_POLL);
    cx.run_until_parked();
    let toolbar = if TOOLBAR {
        TOOLBAR_HEIGHT + px(1.)
    } else {
        px(0.)
    };
    let fitted = window
        .update(cx, |_, window, _| window.bounds().size)
        .unwrap();
    assert_eq!(
        fitted,
        size(
            px(160.).max(WINDOW_MIN_SIZE.width),
            (px(120.) + toolbar).max(WINDOW_MIN_SIZE.height)
        )
    );
}
