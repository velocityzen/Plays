use gpui::{Entity, TestAppContext, VisualTestContext, WindowHandle};

use super::*;
use crate::api::{self, Invocation};
use crate::ui::framing::{ASPECT_RATIOS, CROPS};
use crate::ui::testing::{self, fixture};
use crate::ui::workspace::Workspace;

/// The block `window` has, once it has one and its file has opened.
fn opened(window: WindowHandle<Workspace>, cx: &mut VisualTestContext) -> Entity<PlayerView> {
    let block = |cx: &TestAppContext| {
        window
            .read_with(cx, |workspace, _| workspace.players().first().cloned())
            .unwrap()
    };
    let known = testing::wait_until(cx, |cx| {
        block(cx).is_some_and(|view| view.read_with(cx, |view, _| view.player().is_known()))
    });
    assert!(known, "it opens");
    block(cx).unwrap()
}

/// How `block` shows its picture, and its audio and subtitle delays.
fn set(block: &Entity<PlayerView>, cx: &VisualTestContext) -> (Adjustments, f64, f64) {
    block.read_with(cx, |view, _| {
        let player = view.player();
        (
            view.adjustments(),
            player.audio_delay(),
            player.subtitle_delay(),
        )
    })
}

#[gpui::test]
fn a_file_opened_again_shows_and_syncs_as_it_was_set_to(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let open = |cx: &mut VisualTestContext| {
        window
            .update(cx, |workspace, _, cx| {
                workspace.open_paths(vec![fixture("clip-subs.mkv")], cx)
            })
            .unwrap();
        opened(window, cx)
    };
    let block = open(&mut cx);
    block.update(&mut cx, |view, cx| {
        view.rotate(1, cx);
        view.flip(true, cx);
        view.set_aspect_ratio(Some(ASPECT_RATIOS[0]), cx);
        view.set_crop(Some(CROPS[2]), cx);
        view.set_audio_delay(0.25, cx);
        view.set_subtitle_delay(-0.5, cx);
    });
    let adjusted = set(&block, &cx);
    assert_ne!(adjusted.0, Adjustments::default());
    assert_eq!((adjusted.1, adjusted.2), (0.25, -0.5));

    // Another file in its place shows as it is; this one again, as it was.
    block.update(&mut cx, |view, cx| {
        view.play_instead(fixture("clip.mp4"), cx)
    });
    assert_eq!(set(&block, &cx), (Adjustments::default(), 0.0, 0.0));
    opened(window, &mut cx);
    block.update(&mut cx, |view, cx| {
        view.play_instead(fixture("clip-subs.mkv"), cx)
    });
    assert_eq!(set(&block, &cx), adjusted, "in the same block");

    // In a block of its own.
    opened(window, &mut cx);
    TestAppContext::update(&cx, |cx| api::run(Invocation::new("block.close"), cx)).unwrap();
    cx.run_until_parked();
    let block = open(&mut cx);
    assert_eq!(set(&block, &cx), adjusted, "in a block of its own");
}
