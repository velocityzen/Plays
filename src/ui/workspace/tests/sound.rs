//! A window playing through an audio output of its own, picked from a
//! tile's menu, as someone at the screen would.

use std::sync::Arc;

use gpui::{TestAppContext, VisualTestContext, WindowHandle};

use super::open;
use crate::app::{AudioEngine, SetWindowAudioDevice};
use crate::media::Mixer;
use crate::media::output::DeviceInfo;
use crate::ui::testing::{self, click, fixture, hover, shows, wait_until};
use crate::ui::workspace::Workspace;

/// Every tile's mixer in `window`, and the device it plays through.
fn sound(
    window: WindowHandle<Workspace>,
    cx: &TestAppContext,
) -> (Vec<Arc<Mixer>>, Option<String>) {
    window
        .read_with(cx, |workspace, cx| {
            let mixers = workspace
                .tiles
                .iter()
                .filter_map(|tile| tile.view.read(cx).player().mixer())
                .collect();
            let device = workspace.sound_device().map(|device| device.name.clone());
            (mixers, device)
        })
        .unwrap()
}

#[gpui::test]
fn a_window_plays_through_an_output_of_its_own(cx: &mut TestAppContext) {
    let (window, mut cx) = testing::start(cx);
    open(window, &mut cx, &["clip.mp4"], false);
    let app = cx.update(|_, cx| AudioEngine::mixer(cx)).unwrap();
    let outputs = |cx: &mut VisualTestContext| cx.update(|_, cx| AudioEngine::window_outputs(cx));
    assert!(!shows(&mut cx, "cast"), "nothing to pick, with one output");

    cx.update(|_, cx| {
        let device = |id: &str, name: &str| DeviceInfo {
            id: id.into(),
            name: name.into(),
        };
        AudioEngine::set_devices(vec![device("desk", "Desk"), device("tv", "TV")], cx);
    });
    click(&mut cx, "cast");
    assert!(shows(&mut cx, "sound-app") && shows(&mut cx, "sound-1"));
    assert!(!shows(&mut cx, "cast-here"), "nothing else to play on");

    click(&mut cx, "sound-1");
    cx.run_until_parked();
    let (mixers, device) = sound(window, &cx);
    assert_eq!(device.as_deref(), Some("TV"));
    assert!(!Arc::ptr_eq(&mixers[0], &app), "the tile plays through it");
    assert_eq!(outputs(&mut cx), 1);
    let tv = mixers[0].clone();

    // A file opened in it plays there too.
    window
        .update(&mut cx, |workspace, _, cx| {
            workspace.open_paths(vec![fixture("tone.m4a")], cx)
        })
        .unwrap();
    let opened = wait_until(&mut cx, |cx| sound(window, cx).0.len() == 2);
    assert!(opened, "tone.m4a didn't open");
    let (mixers, _) = sound(window, &cx);
    assert!(mixers.iter().all(|mixer| Arc::ptr_eq(mixer, &tv)));

    // A tile popped out takes the device along, sharing its output.
    let before = cx.windows();
    hover(&mut cx, "tile-0");
    click(&mut cx, "pop-out");
    cx.run_until_parked();
    let popped = cx
        .windows()
        .into_iter()
        .find(|window| !before.contains(window))
        .and_then(|window| window.downcast::<Workspace>())
        .expect("a window for the tile");
    let (mixers, device) = sound(popped, &cx);
    assert_eq!(device.as_deref(), Some("TV"));
    assert!(Arc::ptr_eq(&mixers[0], &tv));
    assert_eq!(outputs(&mut cx), 1, "one output for both");

    // Back to the app's output: the other window keeps the device.
    cx.dispatch_action(SetWindowAudioDevice(None));
    cx.run_until_parked();
    let (mixers, device) = sound(window, &cx);
    assert_eq!(device, None);
    assert!(Arc::ptr_eq(&mixers[0], &app));
    assert_eq!(outputs(&mut cx), 1);

    // Its last window closed, the output closes.
    popped
        .update(&mut cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(outputs(&mut cx), 0);
}
