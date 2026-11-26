use std::path::Path;

use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle, point, px};

use super::*;
use crate::ui::player::osd::Osd;
use crate::ui::testing;
use crate::ui::workspace::Workspace;

/// The block `window` opens `path` in, paused, once it's open and its
/// folder looked through.
fn opened(
    window: WindowHandle<Workspace>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<PlayerView> {
    let paths = vec![path.to_owned()];
    window
        .update(cx, |workspace, _, cx| workspace.open_paths(paths, cx))
        .unwrap();
    let block = |cx: &TestAppContext| {
        window
            .read_with(cx, |workspace, _| workspace.players().first().cloned())
            .unwrap()
    };
    let ready = testing::wait_until(cx, |cx| {
        block(cx).is_some_and(|block| {
            block.read_with(cx, |view, _| {
                view.player().is_known() && view.folder_files().is_some()
            })
        })
    });
    assert!(ready, "{} opens", path.display());
    let block = block(cx).unwrap();
    block.update(cx, |view, cx| {
        view.pause();
        view.seek(0.0);
        cx.notify();
    });
    block
}

/// What skipping `block` does once it's at `time`.
fn skip_at(block: &Entity<PlayerView>, time: f64, cx: &mut VisualTestContext) -> Option<Skip> {
    block.update(cx, |view, cx| {
        view.seek(time);
        cx.notify();
    });
    let there = testing::wait_until(cx, |cx| {
        block.read_with(cx, |view, _| (view.position() - time).abs() < 0.05)
    });
    assert!(
        there,
        "at {}",
        block.read_with(cx, |view, _| view.position())
    );
    block.read_with(cx, |view, _| view.skip_now())
}

/// The note `block` shows, if any.
fn noted(block: &Entity<PlayerView>, cx: &VisualTestContext) -> Option<String> {
    block.read_with(cx, |view, _| match &view.osd {
        Some((Osd::Text(text), _)) => Some(text.to_string()),
        _ => None,
    })
}

#[gpui::test]
fn skips_the_intro_and_past_the_credits_to_the_next_episode(cx: &mut TestAppContext) {
    let (first, second) = testing::episodes();
    let (window, mut cx) = testing::start(cx);
    let block = opened(window, &first, &mut cx);

    assert_eq!(
        skip_at(&block, 0.0, &mut cx),
        Some(Skip::Past(Part::Intro, 1.0))
    );
    assert_eq!(Skip::Past(Part::Intro, 1.0).label(), "Skip Intro");
    assert_eq!(Skip::Past(Part::Credits, 6.0).label(), "Skip Credits");
    // A press on it doesn't drag the window by the picture under it.
    testing::drag_by(&mut cx, "skip", point(px(-40.), px(-30.)));
    assert_eq!(block.read_with(&cx, |view, _| view.window_moves), 0);
    skip_at(&block, 0.0, &mut cx);
    testing::click(&mut cx, "skip");
    let past = testing::wait_until(&mut cx, |cx| {
        block.read_with(cx, |view, _| (view.position() - 1.0).abs() < 0.05)
    });
    assert!(past, "past the intro");
    assert!(
        !testing::shows(&mut cx, "skip"),
        "its story: nothing to skip"
    );
    block.update(&mut cx, |view, cx| view.skip(cx));
    assert_eq!(noted(&block, &cx).as_deref(), Some("Nothing to skip"));

    // Credits it ends with: the next file in its folder.
    assert_eq!(skip_at(&block, 2.2, &mut cx), Some(Skip::NextFile));
    assert_eq!(Skip::NextFile.label(), "Next Episode");
    testing::click(&mut cx, "skip");
    let next = testing::wait_until(&mut cx, |cx| {
        block.read_with(cx, |view, _| view.file() == Some(second.as_path()))
    });
    assert!(next, "the next episode plays");
}

#[gpui::test]
fn credits_a_last_episode_ends_with_are_left_to_play(cx: &mut TestAppContext) {
    let last = testing::episode("last-episode");
    let (window, mut cx) = testing::start(cx);
    let block = opened(window, &last, &mut cx);
    assert_eq!(skip_at(&block, 2.2, &mut cx), None);
    assert!(!testing::shows(&mut cx, "skip"));
}
