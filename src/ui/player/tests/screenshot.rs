use std::fs;

use gpui::TestAppContext;

use crate::files::test_folder;
use crate::media::Source;
use crate::media::tests::{http, stream_media};
use crate::settings::Settings;
use crate::ui::testing;

#[gpui::test]
fn a_live_stream_is_screenshotted_as_it_shows(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    let folder = test_folder("live-screenshots");
    let _ = fs::remove_dir_all(&folder);
    cx.update(|_, cx| Settings::update(cx, false, |s| s.screenshots.folder = Some(folder.clone())));
    let served = test_folder("live-screenshotted");
    let _ = fs::remove_dir_all(&served);
    fs::create_dir_all(&served).unwrap();
    for file in fs::read_dir(stream_media().join("hls")).unwrap() {
        let file = file.unwrap().path();
        fs::copy(&file, served.join(file.file_name().unwrap())).unwrap();
    }
    let addr = http::serve(served.clone(), None);
    let source = Source::parse_url(&format!("http://{addr}/live.m3u8")).unwrap();
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_sources(vec![source], cx)
        })
        .unwrap();
    let players = |cx: &TestAppContext| {
        window
            .read_with(cx, |workspace, _| workspace.players())
            .unwrap()
    };
    assert!(
        testing::wait_until(&mut cx, |cx| players(cx).len() == 1),
        "it opens"
    );
    let block = players(&cx).remove(0);
    let showing = testing::wait_until(&mut cx, |cx| {
        testing::all_started(window, 1, cx)
            && block.read_with(cx, |view, _| view.player().frame().is_some())
    });
    assert!(showing, "it plays");
    let size = block.read_with(&cx, |view, _| {
        assert!(view.player().is_live());
        view.player().frame().unwrap().size()
    });

    // Gone from the server, as a live stream's past is: it can't be opened
    // again where it was.
    fs::remove_file(served.join("live.m3u8")).unwrap();
    block.update_in(&mut cx, |view, window, cx| view.take_screenshot(window, cx));
    let saved = || fs::read_dir(&folder).map_or(0, |files| files.count());
    let said = testing::wait_until(&mut cx, |cx| {
        saved() == 1 && block.read_with(cx, |view, _| view.noted().is_some())
    });
    let note = block.read_with(&cx, |view, _| view.noted());
    assert!(said, "saved: {note:?}");
    let note = note.unwrap();
    assert!(note.starts_with("Screenshot saved"), "{note}");
    let file = fs::read_dir(&folder)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let shot = image::open(&file).unwrap();
    assert_eq!(
        (shot.width(), shot.height()),
        size,
        "at the size it plays at"
    );
    let _ = fs::remove_dir_all(&folder);
    let _ = fs::remove_dir_all(&served);
}
